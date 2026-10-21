//! Local inter-process communication for the AivoBlock node.
//!
//! The Rust node will call the Haskell validator and Python AI worker as
//! separate local processes. This module defines a bounded binary frame:
//! four-byte big-endian payload length followed by payload bytes.
//! It does not authenticate remote network traffic or define canonical
//! blockchain serialization.

pub mod haskell;
pub mod ledger_responses;
pub mod python;

use std::io;
use std::process::{Child, ChildStdin, ChildStdout};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Maximum permitted payload size for one local IPC message (4 MiB).
pub const MAX_IPC_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

/// Run one request/response `exchange` over a spawned child's pipes on a
/// helper thread, and wait at most `timeout` for it. On expiry the child is
/// killed, which closes its pipes and lets the helper finish on its own, so
/// a hung or stalled local executable cannot block the caller (for example
/// the consensus event loop or a worker slot) indefinitely.
///
/// Returns None on timeout. The caller still reaps the child with `wait`.
pub fn exchange_within<T, E>(
    child: &mut Child,
    timeout: Duration,
    exchange: impl FnOnce(ChildStdin, ChildStdout) -> Result<T, E> + Send + 'static,
) -> Option<Result<T, E>>
where
    T: Send + 'static,
    E: From<io::Error> + Send + 'static,
{
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Some(Err(io::Error::other("missing child pipes").into()));
    };
    let (sender, receiver) = mpsc::channel();
    let helper = thread::spawn(move || {
        let _ = sender.send(exchange(stdin, stdout));
    });
    match receiver.recv_timeout(timeout) {
        Ok(result) => {
            let _ = helper.join();
            Some(result)
        }
        Err(_) => {
            // Do not join: a grandchild could still hold the pipes open.
            let _ = child.kill();
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    EmptyPayload,
    PayloadTooLarge,
    IncompleteHeader,
    IncompletePayload,
    TrailingBytes,
}

/// Encode exactly one length-prefixed message.
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.is_empty() {
        return Err(FrameError::EmptyPayload);
    }
    if payload.len() > MAX_IPC_PAYLOAD_BYTES {
        return Err(FrameError::PayloadTooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| FrameError::PayloadTooLarge)?;
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Decode exactly one complete frame from an in-memory buffer.
pub fn decode_frame(frame: &[u8]) -> Result<&[u8], FrameError> {
    if frame.len() < 4 {
        return Err(FrameError::IncompleteHeader);
    }
    let length =
        u32::from_be_bytes(frame[..4].try_into().expect("checked four-byte header")) as usize;
    if length == 0 {
        return Err(FrameError::EmptyPayload);
    }
    if length > MAX_IPC_PAYLOAD_BYTES {
        return Err(FrameError::PayloadTooLarge);
    }
    let expected = length + 4;
    if frame.len() < expected {
        return Err(FrameError::IncompletePayload);
    }
    if frame.len() > expected {
        return Err(FrameError::TrailingBytes);
    }
    Ok(&frame[4..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_binary_payloads() {
        let payload = [0, 1, 255, 42];
        let frame = encode_frame(&payload).unwrap();
        assert_eq!(decode_frame(&frame), Ok(payload.as_slice()));
    }

    #[test]
    fn rejects_invalid_frames() {
        assert_eq!(encode_frame(b""), Err(FrameError::EmptyPayload));
        assert_eq!(decode_frame(&[0, 0]), Err(FrameError::IncompleteHeader));
        assert_eq!(decode_frame(&[0, 0, 0, 0]), Err(FrameError::EmptyPayload));
        assert_eq!(
            decode_frame(&[0, 0, 0, 2, 1]),
            Err(FrameError::IncompletePayload)
        );
        assert_eq!(
            decode_frame(&[0, 0, 0, 1, 1, 2]),
            Err(FrameError::TrailingBytes)
        );
    }

    #[cfg(unix)]
    #[test]
    fn exchange_within_returns_a_prompt_answer() {
        use std::io::{Read, Write};
        use std::process::{Command, Stdio};

        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let echo = |mut stdin: ChildStdin, mut stdout: ChildStdout| {
            stdin.write_all(b"ping")?;
            drop(stdin);
            let mut echoed = Vec::new();
            stdout.read_to_end(&mut echoed)?;
            Ok::<_, io::Error>(echoed)
        };
        let echoed = exchange_within(&mut child, Duration::from_secs(10), echo);
        assert_eq!(echoed.unwrap().unwrap(), b"ping");
        assert!(child.wait().unwrap().success());
    }

    /// A child that never answers is killed at the deadline instead of
    /// blocking the caller (for example the consensus event loop).
    #[cfg(unix)]
    #[test]
    fn exchange_within_kills_a_child_that_never_answers() {
        use std::io::Read;
        use std::process::{Command, Stdio};
        use std::time::Instant;

        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let read_one = |_: ChildStdin, mut stdout: ChildStdout| {
            let mut byte = [0u8; 1];
            stdout.read_exact(&mut byte).map(|()| byte[0])
        };
        let outcome = exchange_within(&mut child, Duration::from_millis(200), read_one);
        assert!(outcome.is_none());
        assert!(!child.wait().unwrap().success());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn rejects_oversized_payloads() {
        let payload = vec![0; MAX_IPC_PAYLOAD_BYTES + 1];
        assert_eq!(encode_frame(&payload), Err(FrameError::PayloadTooLarge));
        let length = (MAX_IPC_PAYLOAD_BYTES as u32 + 1).to_be_bytes();
        assert_eq!(decode_frame(&length), Err(FrameError::PayloadTooLarge));
    }
}
