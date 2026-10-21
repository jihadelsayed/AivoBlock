//! Peer-to-peer messages: JSON envelopes over the bounded 4-byte framing in
//! `network::tcp`. Consensus-critical content travels as hex of canonical
//! binary encodings and is individually authenticated (authority-signed
//! tasks, worker-signed claims, validator-signed votes, self-verifying
//! blocks plus quorum certificates). Live peer frames additionally carry a
//! session-bound Ed25519 signature and strictly increasing sequence number.
//! Signed frames are carried inside the mandatory encrypted P2P channel.

use std::io;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use super::{hex, unhex, unhex_array};
use crate::ai_block_v3::{WorkClaimV3, MAX_BLOCK_BYTES};
use crate::network::tcp::{recv_frame, send_frame};
use crate::validator_certificate::SignedPrecommitV1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireMessage {
    Status {
        height: u64,
        tip: String,
    },
    Task {
        task: String,
    },
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
    Commit {
        block: String,
        certificate: String,
    },
    GetBlocks {
        from_height: u64,
    },
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

    /// Upper estimate of the encoded JSON size, used for outbound queue
    /// budgets: every variable field is ASCII hex, plus a fixed envelope.
    pub fn approx_bytes(&self) -> usize {
        let fields = match self {
            Self::Status { tip, .. } => tip.len(),
            Self::Task { task } => task.len(),
            Self::Claim {
                task_id,
                worker,
                result,
                signature,
                ..
            } => task_id.len() + worker.len() + result.len() + signature.len(),
            Self::Proposal { block, .. } => block.len(),
            Self::Vote {
                block_id,
                validator,
                signature,
                ..
            } => block_id.len() + validator.len() + signature.len(),
            Self::Commit { block, certificate } => block.len() + certificate.len(),
            Self::GetBlocks { .. } => 0,
        };
        fields + 256
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

const FRAME_DOMAIN: &[u8] = b"AIVOBLOCK-SIGNED-PEER-FRAME-V1\0";
const FRAME_VERSION: u8 = 1;
const SIGNATURE_BYTES: usize = 64;
const FRAME_PREFIX: usize = 1 + 8 + SIGNATURE_BYTES;

/// Domain-separated, session-bound, direction-bound signing bytes.
fn signed_preimage(
    chain_id: u32,
    session_id: &[u8; 32],
    sender: &[u8; 32],
    recipient: &[u8; 32],
    sequence: u64,
    payload: &[u8],
) -> io::Result<Vec<u8>> {
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Oversized signed payload"))?;
    let mut bytes = Vec::with_capacity(FRAME_DOMAIN.len() + 4 + 32 * 3 + 8 + 4 + payload.len());
    bytes.extend_from_slice(FRAME_DOMAIN);
    bytes.extend_from_slice(&chain_id.to_be_bytes());
    bytes.extend_from_slice(session_id);
    bytes.extend_from_slice(sender);
    bytes.extend_from_slice(recipient);
    bytes.extend_from_slice(&sequence.to_be_bytes());
    bytes.extend_from_slice(&len.to_be_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}

/// Send an authenticated frame: version:u8 | sequence:u64 | signature:64 | JSON.
/// Sequence numbers must start at 1 and increase by exactly one per session.
/// The calling link's single writer thread owns the counter.
pub fn send_authenticated_message<W: io::Write>(
    stream: &mut W,
    message: &WireMessage,
    local: &SigningKey,
    recipient: &[u8; 32],
    chain_id: u32,
    session_id: &[u8; 32],
    sequence: u64,
) -> io::Result<()> {
    if sequence == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Signed-frame sequence must start at one",
        ));
    }
    let payload = serde_json::to_vec(message).map_err(io::Error::other)?;
    let signed = signed_preimage(
        chain_id,
        session_id,
        &local.verifying_key().to_bytes(),
        recipient,
        sequence,
        &payload,
    )?;
    let signature = local.sign(&signed).to_bytes();
    let mut frame = Vec::with_capacity(FRAME_PREFIX + payload.len());
    frame.push(FRAME_VERSION);
    frame.extend_from_slice(&sequence.to_be_bytes());
    frame.extend_from_slice(&signature);
    frame.extend_from_slice(&payload);
    send_frame(stream, &frame)
}

/// Receive a frame only if its signature matches the authenticated peer,
/// session, chain, receiver and the next expected sequence number.
/// On any error, the caller must terminate this peer connection.
pub fn recv_authenticated_message<R: io::Read>(
    stream: &mut R,
    peer: &[u8; 32],
    local: &[u8; 32],
    chain_id: u32,
    session_id: &[u8; 32],
    last_sequence: &mut u64,
) -> io::Result<WireMessage> {
    let frame = recv_frame(stream)?;
    if frame.len() <= FRAME_PREFIX || frame[0] != FRAME_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid signed-frame version or length",
        ));
    }
    let sequence = u64::from_be_bytes(frame[1..9].try_into().unwrap());
    let expected = last_sequence.checked_add(1).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Signed-frame sequence overflow")
    })?;
    if sequence != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Replayed or out-of-order signed frame",
        ));
    }
    let signature: [u8; SIGNATURE_BYTES] = frame[9..FRAME_PREFIX].try_into().unwrap();
    let payload = &frame[FRAME_PREFIX..];
    let signed = signed_preimage(chain_id, session_id, peer, local, sequence, payload)?;
    let verifying_key = VerifyingKey::from_bytes(peer)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid configured peer key"))?;
    verifying_key
        .verify_strict(&signed, &Signature::from_bytes(&signature))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid peer-frame signature"))?;
    let message = serde_json::from_slice(payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    *last_sequence = sequence;
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn message() -> WireMessage {
        WireMessage::Proposal {
            height: 3,
            round: 1,
            block: "00ff".into(),
        }
    }

    fn signed_frame(
        signer: &SigningKey,
        recipient: &[u8; 32],
        session: &[u8; 32],
        sequence: u64,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        send_authenticated_message(
            &mut out,
            &message(),
            signer,
            recipient,
            1,
            session,
            sequence,
        )
        .unwrap();
        out
    }

    #[test]
    fn authenticated_frames_round_trip_and_advance_sequence() {
        let alice = SigningKey::from_bytes(&[1; 32]);
        let bob = SigningKey::from_bytes(&[2; 32]);
        let session = [3u8; 32];
        let mut frames = Vec::new();
        for sequence in 1..=2 {
            send_authenticated_message(
                &mut frames,
                &message(),
                &alice,
                &bob.verifying_key().to_bytes(),
                1,
                &session,
                sequence,
            )
            .unwrap();
        }
        let mut cursor = Cursor::new(frames);
        let mut last = 0;
        for sequence in 1..=2 {
            assert_eq!(
                recv_authenticated_message(
                    &mut cursor,
                    &alice.verifying_key().to_bytes(),
                    &bob.verifying_key().to_bytes(),
                    1,
                    &session,
                    &mut last,
                )
                .unwrap(),
                message()
            );
            assert_eq!(last, sequence);
        }
    }

    #[test]
    fn signed_frames_reject_tampering_forgery_cross_session_and_replay() {
        let alice = SigningKey::from_bytes(&[1; 32]);
        let bob = SigningKey::from_bytes(&[2; 32]);
        let mallory = SigningKey::from_bytes(&[9; 32]);
        let sender = alice.verifying_key().to_bytes();
        let receiver = bob.verifying_key().to_bytes();
        let session = [3u8; 32];
        let valid = signed_frame(&alice, &receiver, &session, 1);
        let recv = |frame: Vec<u8>,
                    claimed_sender: &[u8; 32],
                    local: &[u8; 32],
                    chain: u32,
                    session_id: &[u8; 32],
                    last: &mut u64| {
            recv_authenticated_message(
                &mut Cursor::new(frame),
                claimed_sender,
                local,
                chain,
                session_id,
                last,
            )
        };
        // Valid frame, replay at sequence 1, wrong chain, wrong recipient,
        // different handshake session, unknown signer, and skipped sequence.
        let mut last = 0;
        assert!(recv(valid.clone(), &sender, &receiver, 1, &session, &mut last).is_ok());
        assert!(recv(valid.clone(), &sender, &receiver, 1, &session, &mut last).is_err());
        assert_eq!(last, 1);
        assert!(recv(valid.clone(), &sender, &receiver, 2, &session, &mut 0).is_err());
        assert!(recv(valid.clone(), &sender, &sender, 1, &session, &mut 0).is_err());
        assert!(recv(valid.clone(), &sender, &receiver, 1, &[4; 32], &mut 0).is_err());
        assert!(recv(
            signed_frame(&mallory, &receiver, &session, 1),
            &sender,
            &receiver,
            1,
            &session,
            &mut 0
        )
        .is_err());
        assert!(recv(
            signed_frame(&alice, &receiver, &session, 2),
            &sender,
            &receiver,
            1,
            &session,
            &mut 0
        )
        .is_err());

        for index in [4usize, 12, 80, valid.len() - 1] {
            let mut changed = valid.clone();
            changed[index] ^= 1;
            assert!(recv(changed, &sender, &receiver, 1, &session, &mut 0).is_err());
        }
        let mut wrong_version = valid.clone();
        wrong_version[4] = 99;
        assert!(recv(wrong_version, &sender, &receiver, 1, &session, &mut 0).is_err());
        assert!(recv(
            valid[..valid.len() - 1].to_vec(),
            &sender,
            &receiver,
            1,
            &session,
            &mut 0
        )
        .is_err());
        assert!(send_authenticated_message(
            &mut Vec::new(),
            &message(),
            &alice,
            &receiver,
            1,
            &session,
            0
        )
        .is_err());
    }
}
