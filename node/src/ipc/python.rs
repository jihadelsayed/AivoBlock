//! IPC adapter for a local Python AI puzzle worker.
//!
//! The worker protocol uses a 4-byte big-endian length prefix, followed
//! by opaque bytes. Python must implement the same framing rules.
//! This adapter does not verify AI proofs or grant consensus authority.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{encode_frame, FrameError, MAX_IPC_PAYLOAD_BYTES};

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
}

impl std::fmt::Display for PythonIpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Framing(error) => write!(f, "invalid Python IPC frame: {error:?}"),
            Self::Io(error) => write!(f, "Python worker I/O error: {error}"),
            Self::ProcessFailed(code) => {
                write!(f, "Python worker exited with status {code:?}")
            }
            error => write!(f, "{error:?}"),
        }
    }
}

impl std::error::Error for PythonIpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
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

/// Launches a configured local Python worker for one request.
///
/// Use trusted executable and script paths only. A later implementation
/// should add process supervision and request timeouts.
#[derive(Debug, Clone)]
pub struct PythonWorkerClient {
    python_executable: PathBuf,
    worker_script: PathBuf,
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
        })
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

        let response = (|| {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| std::io::Error::other("missing worker stdin"))?;
            stdin.write_all(&frame)?;
            drop(stdin);

            let mut stdout = child
                .stdout
                .take()
                .ok_or_else(|| std::io::Error::other("missing worker stdout"))?;
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
        })();

        if response.is_err() {
            let _ = child.kill();
        }
        let status = child.wait()?;
        if !status.success() {
            return Err(PythonIpcError::ProcessFailed(status.code()));
        }
        response
    }
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

    #[test]
    fn rejects_empty_request_before_launching() {
        let client = PythonWorkerClient::new("python", "worker.py").unwrap();
        assert!(matches!(
            client.exchange(b""),
            Err(PythonIpcError::Framing(FrameError::EmptyPayload))
        ));
    }
}
