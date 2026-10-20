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

/// Maximum permitted payload size for one local IPC message (4 MiB).
pub const MAX_IPC_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

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

    #[test]
    fn rejects_oversized_payloads() {
        let payload = vec![0; MAX_IPC_PAYLOAD_BYTES + 1];
        assert_eq!(encode_frame(&payload), Err(FrameError::PayloadTooLarge));
        let length = (MAX_IPC_PAYLOAD_BYTES as u32 + 1).to_be_bytes();
        assert_eq!(decode_frame(&length), Err(FrameError::PayloadTooLarge));
    }
}
