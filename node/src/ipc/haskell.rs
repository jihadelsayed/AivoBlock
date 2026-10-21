//! Local framed IPC client for AivoBlock Haskell diagnostics and structural checks.
//!
//! The executable must implement the binary framing contract defined in
//! `ipc::mod`: a 4-byte big-endian length followed by a payload.
//! The Haskell CLI answers one request per process with `ipc-health`, or
//! many requests over one long-lived process with `ipc-serve` (persistent
//! mode, used by the testnet runtime to avoid starting the GHC runtime for
//! every block). Structural acceptance is not full consensus validation.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::{encode_frame, exchange_within, FrameError, MAX_IPC_PAYLOAD_BYTES};

/// Wall-clock bound for one Haskell request, from spawn to the final byte.
/// A settlement takes milliseconds; a hung core must not stall consensus.
pub const DEFAULT_HASKELL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum HaskellIpcError {
    InvalidExecutable,
    Framing(FrameError),
    Io(std::io::Error),
    ProcessFailed(Option<i32>),
    EmptyResponse,
    ResponseTooLarge,
    IncompleteResponse,
    TimedOut,
}

impl From<std::io::Error> for HaskellIpcError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<FrameError> for HaskellIpcError {
    fn from(error: FrameError) -> Self {
        Self::Framing(error)
    }
}

/// One long-lived `ipc-serve` child. A writer thread feeds it request frames
/// and a reader thread returns response frames, so every call waits with a
/// deadline even if the child stops reading or writing. Dropping the session
/// kills and reaps the child.
#[derive(Debug)]
struct Session {
    child: Child,
    requests: SyncSender<Vec<u8>>,
    responses: Receiver<Reply>,
}

type Reply = Result<Vec<u8>, HaskellIpcError>;
type SharedSession = Arc<Mutex<Option<Session>>>;

impl Session {
    fn spawn(executable: &Path, args: &[&str]) -> Result<Self, HaskellIpcError> {
        let mut child = Command::new(executable)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let (Some(mut stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HaskellIpcError::Io(std::io::Error::other("missing child pipes")));
        };
        let (requests, pending) = mpsc::sync_channel::<Vec<u8>>(1);
        let (answers, responses) = mpsc::channel();
        thread::spawn(move || {
            for frame in pending {
                let written = stdin.write_all(&frame).and_then(|()| stdin.flush());
                if written.is_err() {
                    break;
                }
            }
        });
        thread::spawn(move || loop {
            let response = read_response(&mut stdout);
            let failed = response.is_err();
            if answers.send(response).is_err() || failed {
                break;
            }
        });
        Ok(Self {
            child,
            requests,
            responses,
        })
    }

    /// One request and its response within `timeout`. Calls are serialized
    /// by the client, so at most one request is ever outstanding.
    fn call(&self, frame: Vec<u8>, timeout: Duration) -> Result<Vec<u8>, HaskellIpcError> {
        self.requests
            .send(frame)
            .map_err(|_| HaskellIpcError::IncompleteResponse)?;
        match self.responses.recv_timeout(timeout) {
            Ok(response) => response,
            Err(RecvTimeoutError::Timeout) => Err(HaskellIpcError::TimedOut),
            Err(RecvTimeoutError::Disconnected) => Err(HaskellIpcError::IncompleteResponse),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A request/response adapter with a per-request deadline: by default one
/// child process per request (`ipc-health`), or one supervised long-lived
/// child (`ipc-serve`) in persistent mode.
///
/// Only configure a trusted, locally installed executable. No shell is
/// involved. For production, replace this with a versioned typed protocol.
#[derive(Debug, Clone)]
pub struct HaskellValidatorClient {
    executable: PathBuf,
    timeout: Duration,
    /// Persistent mode: one `ipc-serve` session shared by all clones.
    session: Option<SharedSession>,
}

impl HaskellValidatorClient {
    pub fn new(executable: impl AsRef<Path>) -> Result<Self, HaskellIpcError> {
        let executable = executable.as_ref();
        if executable.as_os_str().is_empty() {
            return Err(HaskellIpcError::InvalidExecutable);
        }
        Ok(Self {
            executable: executable.to_path_buf(),
            timeout: DEFAULT_HASKELL_TIMEOUT,
            session: None,
        })
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Reuse one supervised `aivoblock-core ipc-serve` process for every
    /// request instead of starting the GHC runtime per request. Requests are
    /// serialized; any error, timeout or malformed reply kills that process
    /// and the next request starts a fresh one, so a session is never reused
    /// after a failure. Settlement results are byte-identical either way.
    pub fn persistent(mut self) -> Self {
        self.session = Some(Arc::new(Mutex::new(None)));
        self
    }

    /// Send one length-prefixed request and return the response frame.
    /// Supports health, structural block checks and ledger settlement.
    ///
    /// A successful IPC exchange is NOT evidence of a valid block;
    /// callers must decode and authenticate the validator's verdict.
    pub fn exchange(&self, payload: &[u8]) -> Result<Vec<u8>, HaskellIpcError> {
        let frame = encode_frame(payload)?;
        match &self.session {
            Some(session) => self.exchange_persistent(session, frame),
            None => self.exchange_once(frame),
        }
    }

    fn exchange_persistent(
        &self,
        session: &Mutex<Option<Session>>,
        frame: Vec<u8>,
    ) -> Result<Vec<u8>, HaskellIpcError> {
        let mut slot = session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let live = match slot.take() {
            Some(live) => live,
            None => Session::spawn(&self.executable, &["ipc-serve"])?,
        };
        let outcome = live.call(frame, self.timeout);
        // Only a session that just answered successfully is kept; on any
        // failure `live` is dropped here, which kills and reaps the child.
        if outcome.is_ok() {
            *slot = Some(live);
        }
        outcome
    }

    /// One `aivoblock-core ipc-health` process for this request.
    fn exchange_once(&self, frame: Vec<u8>) -> Result<Vec<u8>, HaskellIpcError> {
        let mut child = Command::new(&self.executable)
            .arg("ipc-health")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let exchange = move |stdin, stdout| exchange_frame(&frame, stdin, stdout);
        let exchanged = exchange_within(&mut child, self.timeout, exchange);
        let result = exchanged.unwrap_or(Err(HaskellIpcError::TimedOut));

        if result.is_err() {
            let _ = child.kill();
        }
        let status = child.wait()?;
        // Keep the framing error when we terminated the child after detecting it.
        // Otherwise report a nonzero exit from the Haskell executable.
        let response = result?;
        if !status.success() {
            return Err(HaskellIpcError::ProcessFailed(status.code()));
        }
        Ok(response)
    }
}

fn read_exact_or_incomplete(
    stdout: &mut ChildStdout,
    buf: &mut [u8],
) -> Result<(), HaskellIpcError> {
    stdout.read_exact(buf).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            HaskellIpcError::IncompleteResponse
        } else {
            HaskellIpcError::Io(error)
        }
    })
}

/// Read exactly one bounded response frame.
fn read_response(stdout: &mut ChildStdout) -> Result<Vec<u8>, HaskellIpcError> {
    let mut header = [0u8; 4];
    read_exact_or_incomplete(stdout, &mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 {
        return Err(HaskellIpcError::EmptyResponse);
    }
    if length > MAX_IPC_PAYLOAD_BYTES {
        return Err(HaskellIpcError::ResponseTooLarge);
    }
    let mut response = vec![0u8; length];
    read_exact_or_incomplete(stdout, &mut response)?;
    Ok(response)
}

/// Write one request frame, then read exactly one response frame and EOF.
fn exchange_frame(
    frame: &[u8],
    mut stdin: ChildStdin,
    mut stdout: ChildStdout,
) -> Result<Vec<u8>, HaskellIpcError> {
    stdin.write_all(frame)?;
    drop(stdin);

    let response = read_response(&mut stdout)?;
    let mut trailing = [0u8; 1];
    if stdout.read(&mut trailing)? != 0 {
        return Err(HaskellIpcError::Framing(FrameError::TrailingBytes));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cat` echoes every request frame back as a response frame, which
    /// exercises the persistent framing without the Haskell binary.
    #[cfg(unix)]
    #[test]
    fn persistent_session_reuses_one_process_and_times_out_a_stalled_one() {
        let session = Session::spawn(Path::new("cat"), &[]).unwrap();
        let pid = session.child.id();
        for payload in [&b"first"[..], &b"second"[..], &b"third"[..]] {
            let frame = encode_frame(payload).unwrap();
            let echoed = session.call(frame, Duration::from_secs(10)).unwrap();
            assert_eq!(echoed, payload);
        }
        assert_eq!(session.child.id(), pid);
        drop(session);

        let stalled = Session::spawn(Path::new("sleep"), &["30"]).unwrap();
        let started = std::time::Instant::now();
        let frame = encode_frame(b"ping").unwrap();
        let outcome = stalled.call(frame, Duration::from_millis(200));
        assert!(matches!(outcome, Err(HaskellIpcError::TimedOut)));
        drop(stalled);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// CI "Rust <-> Haskell" job: the real core answers many requests in one
    /// `ipc-serve` process and a bad request only replaces that session.
    #[test]
    fn persistent_core_answers_repeatedly_and_recovers_after_a_bad_request() {
        let path = std::env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE").unwrap_or_default();
        if path.is_empty() {
            eprintln!("SKIP: AIVOBLOCK_HASKELL_EXECUTABLE is not set");
            return;
        }
        let client = HaskellValidatorClient::new(path).unwrap().persistent();
        let health = br#"{"protocol_version":1,"type":"health"}"#;
        let healthy = |reply: Vec<u8>| String::from_utf8_lossy(&reply).contains("\"ok\":true");
        for _ in 0..5 {
            assert!(healthy(client.exchange(health).unwrap()));
        }
        let _ = client.exchange(b"not json");
        assert!(healthy(client.exchange(health).unwrap()));
    }

    #[test]
    fn persistent_client_reports_a_missing_executable_and_recovers_state() {
        let client = HaskellValidatorClient::new("aivoblock-core-that-does-not-exist")
            .unwrap()
            .persistent();
        for _ in 0..2 {
            assert!(matches!(
                client.exchange(b"{}"),
                Err(HaskellIpcError::Io(_))
            ));
        }
        let slot = client.session.as_ref().unwrap().lock().unwrap();
        assert!(slot.is_none());
    }

    #[test]
    fn rejects_empty_executable_path() {
        assert!(matches!(
            HaskellValidatorClient::new(""),
            Err(HaskellIpcError::InvalidExecutable)
        ));
    }

    #[test]
    fn rejects_request_above_frame_limit_before_spawning() {
        let client = HaskellValidatorClient::new("not-a-real-validator").unwrap();
        let oversized = vec![b'x'; MAX_IPC_PAYLOAD_BYTES + 1];
        assert!(matches!(
            client.exchange(&oversized),
            Err(HaskellIpcError::Framing(_))
        ));
    }

    #[test]
    fn reports_missing_haskell_executable() {
        let client =
            HaskellValidatorClient::new("aivoblock-haskell-executable-that-does-not-exist")
                .unwrap();
        assert!(matches!(
            client.exchange(b"{}"),
            Err(HaskellIpcError::Io(_))
        ));
    }

    #[test]
    fn rejects_empty_request_before_spawning() {
        let client = HaskellValidatorClient::new("not-a-real-validator").unwrap();
        assert!(matches!(
            client.exchange(b""),
            Err(HaskellIpcError::Framing(FrameError::EmptyPayload))
        ));
    }
}
