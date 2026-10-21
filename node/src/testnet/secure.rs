//! Mandatory confidentiality for mutually authenticated validator links.
//!
//! Each connection performs a session-bound, Ed25519-signed ephemeral X25519
//! exchange. HKDF derives *different* ChaCha20-Poly1305 keys for each direction.
//! Frames retain their existing session-bound Ed25519 signature and sequence
//! check inside the AEAD ciphertext. A failed exchange or decryption closes the
//! connection; there is no plaintext fallback.

use std::io::{self, Cursor, Read, Write};

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{x25519, X25519_BASEPOINT_BYTES};

use super::random_bytes32;
use super::wire::{recv_authenticated_message, send_authenticated_message, WireMessage};
use crate::network::tcp::{recv_frame, send_frame};

const EXCHANGE_DOMAIN: &[u8] = b"AIVOBLOCK-ENCRYPTED-HANDSHAKE-V1\0";
const KEY_DOMAIN: &[u8] = b"AIVOBLOCK-DIRECTIONAL-AEAD-V1\0";
const AEAD_DOMAIN: &[u8] = b"AIVOBLOCK-ENCRYPTED-FRAME-V1\0";
const EXCHANGE_BYTES: usize = 32 + 64;
const AEAD_NONCE_BYTES: usize = 12;

#[derive(Clone, Debug)]
pub struct EncryptedChannel {
    send_key: [u8; 32],
    recv_key: [u8; 32],
    chain_id: u32,
    session_id: [u8; 32],
    local: [u8; 32],
    peer: [u8; 32],
}

fn exchange_preimage(
    chain_id: u32,
    session_id: &[u8; 32],
    sender: &[u8; 32],
    recipient: &[u8; 32],
    ephemeral: &[u8; 32],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(EXCHANGE_DOMAIN.len() + 4 + 32 * 4);
    bytes.extend_from_slice(EXCHANGE_DOMAIN);
    bytes.extend_from_slice(&chain_id.to_be_bytes());
    bytes.extend_from_slice(session_id);
    bytes.extend_from_slice(sender);
    bytes.extend_from_slice(recipient);
    bytes.extend_from_slice(ephemeral);
    bytes
}

fn key_info(chain_id: u32, sender: &[u8; 32], recipient: &[u8; 32]) -> Vec<u8> {
    let mut info = Vec::with_capacity(KEY_DOMAIN.len() + 4 + 64);
    info.extend_from_slice(KEY_DOMAIN);
    info.extend_from_slice(&chain_id.to_be_bytes());
    info.extend_from_slice(sender);
    info.extend_from_slice(recipient);
    info
}

/// Exchange authenticated ephemeral X25519 public keys *after* mutual_handshake.
/// The signed transcript includes the prior session ID and both validators,
/// preventing endpoint substitution or key exchange replay across connections.
pub fn negotiate<S: Read + Write>(
    stream: &mut S,
    chain_id: u32,
    session_id: [u8; 32],
    local: &SigningKey,
    peer: [u8; 32],
) -> Result<EncryptedChannel, String> {
    let own = local.verifying_key().to_bytes();
    if chain_id == 0 || session_id == [0; 32] || peer == own {
        return Err("Invalid encrypted-session identity or chain".into());
    }
    let ephemeral_secret = random_bytes32();
    let ephemeral_public = x25519(ephemeral_secret, X25519_BASEPOINT_BYTES);
    let signature = local
        .sign(&exchange_preimage(
            chain_id,
            &session_id,
            &own,
            &peer,
            &ephemeral_public,
        ))
        .to_bytes();
    let mut offer = Vec::with_capacity(EXCHANGE_BYTES);
    offer.extend_from_slice(&ephemeral_public);
    offer.extend_from_slice(&signature);
    send_frame(stream, &offer).map_err(|e| format!("send encrypted key exchange: {e}"))?;
    let response = recv_frame(stream).map_err(|e| format!("receive encrypted key exchange: {e}"))?;
    if response.len() != EXCHANGE_BYTES {
        return Err("Incorrect encrypted key-exchange length".into());
    }
    let theirs: [u8; 32] = response[..32].try_into().unwrap();
    let signature: [u8; 64] = response[32..].try_into().unwrap();
    let verified = VerifyingKey::from_bytes(&peer).map_err(|_| "Invalid peer public key")?;
    verified
        .verify_strict(
            &exchange_preimage(chain_id, &session_id, &peer, &own, &theirs),
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| "Invalid peer encrypted key-exchange signature")?;

    let shared_secret = x25519(ephemeral_secret, theirs);
    if shared_secret == [0; 32] {
        return Err("Invalid low-order peer X25519 key".into());
    }
    let hkdf = Hkdf::<Sha256>::new(Some(&session_id), &shared_secret);
    let mut send_key = [0; 32];
    let mut recv_key = [0; 32];
    hkdf.expand(&key_info(chain_id, &own, &peer), &mut send_key)
        .map_err(|_| "Cannot derive peer send key")?;
    hkdf.expand(&key_info(chain_id, &peer, &own), &mut recv_key)
        .map_err(|_| "Cannot derive peer receive key")?;
    Ok(EncryptedChannel {
        send_key,
        recv_key,
        chain_id,
        session_id,
        local: own,
        peer,
    })
}

fn nonce(sequence: u64) -> Nonce {
    let mut raw = [0u8; AEAD_NONCE_BYTES];
    raw[4..].copy_from_slice(&sequence.to_be_bytes());
    *Nonce::from_slice(&raw)
}

fn additional_data(
    chain_id: u32,
    session_id: &[u8; 32],
    sender: &[u8; 32],
    recipient: &[u8; 32],
    sequence: u64,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(AEAD_DOMAIN.len() + 4 + 32 * 3 + 8);
    out.extend_from_slice(AEAD_DOMAIN);
    out.extend_from_slice(&chain_id.to_be_bytes());
    out.extend_from_slice(session_id);
    out.extend_from_slice(sender);
    out.extend_from_slice(recipient);
    out.extend_from_slice(&sequence.to_be_bytes());
    out
}

impl EncryptedChannel {
    /// The caller holds the writer mutex and advances its counter only when
    /// this function finishes successfully. Failed writes close the connection.
    pub fn send<W: Write>(
        &self,
        stream: &mut W,
        message: &WireMessage,
        signer: &SigningKey,
        sequence: u64,
    ) -> io::Result<()> {
        if sequence == 0 || signer.verifying_key().to_bytes() != self.local {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid encrypted-frame sender or sequence",
            ));
        }
        let mut signed_frame = Vec::new();
        send_authenticated_message(
            &mut signed_frame,
            message,
            signer,
            &self.peer,
            self.chain_id,
            &self.session_id,
            sequence,
        )?;
        let cipher = ChaCha20Poly1305::new((&self.send_key).into());
        let aad = additional_data(
            self.chain_id,
            &self.session_id,
            &self.local,
            &self.peer,
            sequence,
        );
        let ciphertext = cipher
            .encrypt(
                &nonce(sequence),
                Payload {
                    msg: &signed_frame,
                    aad: &aad,
                },
            )
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "AEAD encryption failed"))?;
        send_frame(stream, &ciphertext)
    }

    /// Decrypt using the next expected sequence and verify the inner signature.
    /// Any invalid ciphertext, replay, or malformed inner frame is fatal.
    pub fn recv<R: Read>(
        &self,
        stream: &mut R,
        last_sequence: &mut u64,
    ) -> io::Result<WireMessage> {
        let sequence = last_sequence.checked_add(1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Encrypted-frame sequence overflow")
        })?;
        let ciphertext = recv_frame(stream)?;
        let cipher = ChaCha20Poly1305::new((&self.recv_key).into());
        let aad = additional_data(
            self.chain_id,
            &self.session_id,
            &self.peer,
            &self.local,
            sequence,
        );
        let plaintext = cipher
            .decrypt(
                &nonce(sequence),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid encrypted peer frame"))?;
        let len = plaintext.len() as u64;
        let mut reader = Cursor::new(plaintext);
        let message = recv_authenticated_message(
            &mut reader,
            &self.peer,
            &self.local,
            self.chain_id,
            &self.session_id,
            last_sequence,
        )?;
        if reader.position() != len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unexpected trailing encrypted-frame data",
            ));
        }
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    fn channels() -> (EncryptedChannel, EncryptedChannel, SigningKey, SigningKey) {
        let alice = SigningKey::from_bytes(&[1; 32]);
        let bob = SigningKey::from_bytes(&[2; 32]);
        let session = [3u8; 32];
        let (mut left, mut right) = pair();
        left.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
        right.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
        let a = alice.clone();
        let b = bob.clone();
        let server = thread::spawn(move || {
            negotiate(&mut right, 1, session, &b, a.verifying_key().to_bytes()).unwrap()
        });
        let first = negotiate(&mut left, 1, session, &alice, bob.verifying_key().to_bytes()).unwrap();
        (first, server.join().unwrap(), alice, bob)
    }

    fn proposal() -> WireMessage {
        WireMessage::Proposal {
            height: 3,
            round: 1,
            block: "a1b2c3".into(),
        }
    }

    #[test]
    fn key_agreement_is_directional_and_messages_are_encrypted() {
        let (alice_channel, bob_channel, alice, bob) = channels();
        assert_eq!(alice_channel.send_key, bob_channel.recv_key);
        assert_eq!(alice_channel.recv_key, bob_channel.send_key);
        assert_ne!(alice_channel.send_key, alice_channel.recv_key);

        let mut wire = Vec::new();
        alice_channel.send(&mut wire, &proposal(), &alice, 1).unwrap();
        alice_channel.send(&mut wire, &proposal(), &alice, 2).unwrap();
        assert!(!wire.windows(8).any(|part| part == b"proposal"));
        let mut reader = Cursor::new(wire);
        let mut sequence = 0;
        assert_eq!(bob_channel.recv(&mut reader, &mut sequence).unwrap(), proposal());
        assert_eq!(bob_channel.recv(&mut reader, &mut sequence).unwrap(), proposal());
        assert_eq!(sequence, 2);

        let mut reverse = Vec::new();
        bob_channel.send(&mut reverse, &proposal(), &bob, 1).unwrap();
        assert_eq!(
            alice_channel.recv(&mut Cursor::new(reverse), &mut 0).unwrap(),
            proposal()
        );
    }

    #[test]
    fn encrypted_frames_reject_tampering_replay_and_wrong_keys() {
        let (alice_channel, bob_channel, alice, _) = channels();
        let mut frame = Vec::new();
        alice_channel.send(&mut frame, &proposal(), &alice, 1).unwrap();
        let mut tampered = frame.clone();
        tampered[10] ^= 1;
        assert!(bob_channel.recv(&mut Cursor::new(tampered), &mut 0).is_err());

        let mut double = frame.clone();
        double.extend_from_slice(&frame);
        let mut cursor = Cursor::new(double);
        let mut seen = 0;
        assert!(bob_channel.recv(&mut cursor, &mut seen).is_ok());
        assert!(bob_channel.recv(&mut cursor, &mut seen).is_err());
        assert_eq!(seen, 1);

        // A different chain, sender, or session cannot authenticate ciphertext.
        let mut wrong = bob_channel.clone();
        wrong.chain_id = 2;
        assert!(wrong.recv(&mut Cursor::new(frame.clone()), &mut 0).is_err());
        wrong = bob_channel.clone();
        wrong.session_id = [4u8; 32];
        assert!(wrong.recv(&mut Cursor::new(frame.clone()), &mut 0).is_err());
        wrong = bob_channel.clone();
        wrong.peer = [9u8; 32];
        assert!(wrong.recv(&mut Cursor::new(frame), &mut 0).is_err());
    }

    #[test]
    fn signed_ephemeral_exchange_rejects_unknown_identity() {
        let alice = SigningKey::from_bytes(&[1; 32]);
        let bob = SigningKey::from_bytes(&[2; 32]);
        let wrong = SigningKey::from_bytes(&[9; 32]);
        let (mut left, mut right) = pair();
        left.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
        right.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
        let alice_peer = alice.verifying_key().to_bytes();
        let server = thread::spawn(move || {
            negotiate(&mut right, 1, [3u8; 32], &bob, alice_peer)
        });
        assert!(negotiate(
            &mut left,
            1,
            [3u8; 32],
            &alice,
            wrong.verifying_key().to_bytes(),
        )
        .is_err());
        let _ = server.join().unwrap();
    }
}
