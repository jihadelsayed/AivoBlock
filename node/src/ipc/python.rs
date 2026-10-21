//! IPC adapter for a local Python AI puzzle worker.
//!
//! The worker protocol uses a 4-byte big-endian length prefix, followed
//! by opaque bytes. Python must implement the same framing rules.
//! This adapter does not verify AI proofs or grant consensus authority.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use super::{encode_frame, exchange_within, FrameError, MAX_IPC_PAYLOAD_BYTES};

/// Wall-clock bound for one worker request, from spawn to the final byte.
/// A hung worker is killed so it cannot hold a worker execution slot forever.
pub const DEFAULT_PYTHON_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum PythonIpcError {
    InvalidExecutable,
    InvalidScript,
    Framing(FrameError),
    Io(std::io::Error),
    ProcessFailed(Option<i32>),
    EmptyResponse,
    ResponseTooLarge,
    TrailingBytes,
    TimedOut,
}

impl From<std::io::Error> for PythonIpcError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<FrameError> for PythonIpcError {
    fn from(error: FrameError) -> Self {
        Self::Framing(error)
    }
}

/// Launches a configured local Python worker for one request and kills it
/// if it does not answer within the configured deadline.
///
/// Use trusted executable and script paths only. A later implementation
/// should add persistent process supervision.
#[derive(Debug, Clone)]
pub struct PythonWorkerClient {
    python_executable: PathBuf,
    worker_script: PathBuf,
    timeout: Duration,
}

impl PythonWorkerClient {
    pub fn new(
        python_executable: impl AsRef<Path>,
        worker_script: impl AsRef<Path>,
    ) -> Result<Self, PythonIpcError> {
        if python_executable.as_ref().as_os_str().is_empty() {
            return Err(PythonIpcError::InvalidExecutable);
        }
        if worker_script.as_ref().as_os_str().is_empty() {
            return Err(PythonIpcError::InvalidScript);
        }
        Ok(Self {
            python_executable: python_executable.as_ref().to_path_buf(),
            worker_script: worker_script.as_ref().to_path_buf(),
            timeout: DEFAULT_PYTHON_TIMEOUT,
        })
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Exchanges raw payloads; results remain untrusted until independently
    /// validated against AivoBlock's puzzle protocol.
    pub fn exchange(&self, payload: &[u8]) -> Result<Vec<u8>, PythonIpcError> {
        let frame = encode_frame(payload)?;
        let mut child = Command::new(&self.python_executable)
            .arg(&self.worker_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let exchange = move |stdin, stdout| exchange_frame(&frame, stdin, stdout);
        let exchanged = exchange_within(&mut child, self.timeout, exchange);
        let response = exchanged.unwrap_or(Err(PythonIpcError::TimedOut));

        if response.is_err() {
            let _ = child.kill();
        }
        let status = child.wait()?;
        if matches!(response, Err(PythonIpcError::TimedOut)) {
            return response;
        }
        if !status.success() {
            return Err(PythonIpcError::ProcessFailed(status.code()));
        }
        response
    }
}

/// Write one request frame, then read exactly one response frame and EOF.
fn exchange_frame(
    frame: &[u8],
    mut stdin: ChildStdin,
    mut stdout: ChildStdout,
) -> Result<Vec<u8>, PythonIpcError> {
    stdin.write_all(frame)?;
    drop(stdin);

    let mut header = [0u8; 4];
    stdout.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 {
        return Err(PythonIpcError::EmptyResponse);
    }
    if length > MAX_IPC_PAYLOAD_BYTES {
        return Err(PythonIpcError::ResponseTooLarge);
    }
    let mut body = vec![0u8; length];
    stdout.read_exact(&mut body)?;
    let mut extra = [0u8; 1];
    if stdout.read(&mut extra)? != 0 {
        return Err(PythonIpcError::TrailingBytes);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_paths() {
        assert!(matches!(
            PythonWorkerClient::new("", "worker.py"),
            Err(PythonIpcError::InvalidExecutable)
        ));
        assert!(matches!(
            PythonWorkerClient::new("python", ""),
            Err(PythonIpcError::InvalidScript)
        ));
    }

    /// A worker that never answers is killed at the deadline, releasing the
    /// caller (a worker execution slot) instead of blocking it forever.
    #[cfg(unix)]
    #[test]
    fn hung_worker_is_killed_at_the_deadline() {
        let started = std::time::Instant::now();
        let client = PythonWorkerClient::new("sleep", "30")
            .unwrap()
            .with_timeout(Duration::from_millis(300));
        assert!(matches!(client.exchange(b"{}"), Err(PythonIpcError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn rejects_empty_request_before_launching() {
        let client = PythonWorkerClient::new("python", "worker.py").unwrap();
        assert!(matches!(
            client.exchange(b""),
            Err(PythonIpcError::Framing(FrameError::EmptyPayload))
        ));
    }
}
