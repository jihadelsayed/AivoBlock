//! Peer-to-peer messages: JSON envelopes over the bounded 4-byte framing in
//! `network::tcp`. Consensus-critical content travels as hex of canonical
//! binary encodings and is individually authenticated (authority-signed
//! tasks, worker-signed claims, validator-signed votes, self-verifying
//! blocks plus quorum certificates). The transport itself is not encrypted.

use std::io;

use serde::{Deserialize, Serialize};

use super::{hex, unhex, unhex_array};
use crate::ai_block_v3::{WorkClaimV3, MAX_BLOCK_BYTES};
use crate::network::tcp::{recv_frame, send_frame};
use crate::validator_certificate::SignedPrecommitV1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireMessage {
    Status { height: u64, tip: String },
    Task { task: String },
    Claim {
        task_id: String,
        partition: u32,
        worker: String,
        result: String,
        signature: String,
    },
    Proposal {
        height: u64,
        round: u64,
        block: String,
    },
    Vote {
        height: u64,
        round: u64,
        block_id: String,
        validator: String,
        signature: String,
    },
    Commit { block: String, certificate: String },
    GetBlocks { from_height: u64 },
}

impl WireMessage {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Status { .. } => "status",
            Self::Task { .. } => "task",
            Self::Claim { .. } => "claim",
            Self::Proposal { .. } => "proposal",
            Self::Vote { .. } => "vote",
            Self::Commit { .. } => "commit",
            Self::GetBlocks { .. } => "get_blocks",
        }
    }

    pub fn claim(claim: &WorkClaimV3) -> Self {
        Self::Claim {
            task_id: hex(&claim.task_id),
            partition: claim.partition,
            worker: hex(&claim.worker),
            result: hex(&claim.result_commitment),
            signature: hex(&claim.signature),
        }
    }

    pub fn vote(height: u64, round: u64, block_id: &[u8; 32], vote: &SignedPrecommitV1) -> Self {
        Self::Vote {
            height,
            round,
            block_id: hex(block_id),
            validator: hex(&vote.public_key),
            signature: hex(&vote.signature),
        }
    }
}

pub fn decode_claim(
    task_id: &str,
    partition: u32,
    worker: &str,
    result: &str,
    signature: &str,
) -> Result<WorkClaimV3, String> {
    Ok(WorkClaimV3 {
        task_id: unhex_array(task_id)?,
        partition,
        worker: unhex_array(worker)?,
        result_commitment: unhex_array(result)?,
        signature: unhex_array(signature)?,
    })
}

pub fn decode_block_hex(block: &str) -> Result<Vec<u8>, String> {
    unhex(block, MAX_BLOCK_BYTES)
}

pub fn send_message<W: io::Write>(stream: &mut W, message: &WireMessage) -> io::Result<()> {
    let bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
    send_frame(stream, &bytes)
}

pub fn recv_message<R: io::Read>(stream: &mut R) -> io::Result<WireMessage> {
    let bytes = recv_frame(stream)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn messages_round_trip_through_frames() {
        let message = WireMessage::Proposal {
            height: 3,
            round: 1,
            block: "00ff".into(),
        };
        let mut buffer = Vec::new();
        send_message(&mut buffer, &message).unwrap();
        assert_eq!(recv_message(&mut Cursor::new(buffer)).unwrap(), message);
        let raw = br#"{"type":"get_blocks","from_height":2}"#;
        let mut framed = (raw.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(raw);
        assert_eq!(
            recv_message(&mut Cursor::new(framed)).unwrap(),
            WireMessage::GetBlocks { from_height: 2 }
        );
        let mut bad = 5u32.to_be_bytes().to_vec();
        bad.extend_from_slice(b"{bad}");
        assert!(recv_message(&mut Cursor::new(bad)).is_err());
    }
}
