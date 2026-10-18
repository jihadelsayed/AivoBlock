//! Local framed IPC client for AivoBlock Haskell diagnostics and structural checks.
//!
//! The executable must implement the binary framing contract defined in
//! `ipc::mod`: a 4-byte big-endian length followed by a payload.
//! The Haskell CLI accepts health and structural block checks via `ipc-health`.
//! Structural acceptance is not full consensus validation.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{encode_frame, FrameError, MAX_IPC_PAYLOAD_BYTES};

#[derive(Debug)]
pub enum HaskellIpcError {
    InvalidExecutable,
    Framing(FrameError),
    Io(std::io::Error),
    ProcessFailed(Option<i32>),
    EmptyResponse,
    ResponseTooLarge,
    IncompleteResponse,
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

/// A request/response adapter; one child process per request.
///
/// Only configure a trusted, locally installed executable. No shell is
/// involved. For production, replace this with a persistent supervised
/// worker, strict timeouts, and a versioned typed protocol.
#[derive(Debug, Clone)]
pub struct HaskellValidatorClient {
    executable: PathBuf,
}

impl HaskellValidatorClient {
    pub fn new(executable: impl AsRef<Path>) -> Result<Self, HaskellIpcError> {
        let executable = executable.as_ref();
        if executable.as_os_str().is_empty() {
            return Err(HaskellIpcError::InvalidExecutable);
        }
        Ok(Self {
            executable: executable.to_path_buf(),
        })
    }

    /// Call `aivoblock-core ipc-health` with one length-prefixed request.
    /// Supports health and structural block-check requests.
    /// Send an opaque request and return an opaque response.
    ///
    /// A successful IPC exchange is NOT evidence of a valid block;
    /// callers must decode and authenticate the validator's verdict.
    pub fn exchange(&self, payload: &[u8]) -> Result<Vec<u8>, HaskellIpcError> {
        let frame = encode_frame(payload)?;
        let mut child = Command::new(&self.executable)
            .arg("ipc-health")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let result = (|| {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| HaskellIpcError::Io(std::io::Error::other("missing child stdin")))?;
            stdin.write_all(&frame)?;
            drop(stdin);

            let mut stdout = child.stdout.take().ok_or_else(|| {
                HaskellIpcError::Io(std::io::Error::other("missing child stdout"))
            })?;
            let mut header = [0u8; 4];
            if let Err(error) = stdout.read_exact(&mut header) {
                return if error.kind() == std::io::ErrorKind::UnexpectedEof {
                    Err(HaskellIpcError::IncompleteResponse)
                } else {
                    Err(HaskellIpcError::Io(error))
                };
            }
            let length = u32::from_be_bytes(header) as usize;
            if length == 0 {
                return Err(HaskellIpcError::EmptyResponse);
            }
            if length > MAX_IPC_PAYLOAD_BYTES {
                return Err(HaskellIpcError::ResponseTooLarge);
            }
            let mut response = vec![0u8; length];
            if let Err(error) = stdout.read_exact(&mut response) {
                return if error.kind() == std::io::ErrorKind::UnexpectedEof {
                    Err(HaskellIpcError::IncompleteResponse)
                } else {
                    Err(HaskellIpcError::Io(error))
                };
            }
            let mut trailing = [0u8; 1];
            if stdout.read(&mut trailing)? != 0 {
                return Err(HaskellIpcError::Framing(FrameError::TrailingBytes));
            }
            Ok(response)
        })();

        if result.is_err() {
            let _ = child.kill();
        }
        let status = child.wait()?;
        // Keep the framing error when we terminated the child after detecting it.
        // Otherwise report a nonzero exit from the Haskell executable.
        if result.is_err() {
            return result;
        }
        if !status.success() {
            return Err(HaskellIpcError::ProcessFailed(status.code()));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
