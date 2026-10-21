//! Mutually authenticated TCP links between configured validators.
//!
//! Both sides send a fresh 32-byte challenge, answer the other's challenge
//! with a signed `network::handshake` hello, and accept the peer only if its
//! Ed25519 key is a configured validator (and, when dialing, the expected
//! key for that address). Both challenges and identities are bound to a
//! unique session ID. The second handshake exchanges signed ephemeral X25519
//! keys, deriving directional ChaCha20-Poly1305 secrets. Subsequent frames
//! are encrypted and still carry replay-resistant session-bound signatures.
//! One connection per validator pair: the node with the smaller key dials.

use std::collections::BTreeSet;
use std::io;
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};

use super::node::Event;
use super::random_bytes32;
use super::secure::{negotiate, EncryptedChannel};
use super::wire::WireMessage;
use crate::network::handshake::{sign_hello, verify_hello};
use crate::network::tcp::{configure_stream, recv_frame, send_frame, IO_TIMEOUT};

/// A live link dies when nothing (not even a heartbeat) arrives for this long.
pub const PEER_READ_TIMEOUT: Duration = Duration::from_secs(15);
const DIAL_INTERVAL: Duration = Duration::from_millis(500);

static NEXT_LINK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
struct SignedWriter {
    stream: TcpStream,
    next_sequence: u64,
}

#[derive(Debug, Clone)]
pub struct PeerLink {
    pub key: [u8; 32],
    pub id: u64,
    pub outbound: bool,
    pub remote: String,
    writer: Arc<Mutex<SignedWriter>>,
    local_key: SigningKey,
    channel: EncryptedChannel,
}

impl PeerLink {
    pub fn send(&self, message: &WireMessage) -> io::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("peer writer lock poisoned"))?;
        let sequence = writer.next_sequence;
        let next = sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Peer sequence overflow"))?;
        match self
            .channel
            .send(&mut writer.stream, message, &self.local_key, sequence)
        {
            Ok(()) => {
                writer.next_sequence = next;
                Ok(())
            }
            Err(error) => {
                let _ = writer.stream.shutdown(Shutdown::Both);
                Err(error)
            }
        }
    }

    pub fn close(&self) {
        if let Ok(writer) = self.writer.lock() {
            let _ = writer.stream.shutdown(Shutdown::Both);
        }
    }
}

#[derive(Debug)]
pub struct AuthenticatedSession {
    pub peer: [u8; 32],
    pub session_id: [u8; 32],
}

/// Bind both identities and both independently generated challenges to a
/// single session identifier, independent of which party dialed.
fn session_id(
    chain_id: u32,
    local_key: &[u8; 32],
    local_challenge: &[u8; 32],
    peer_key: &[u8; 32],
    peer_challenge: &[u8; 32],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"AIVOBLOCK-PEER-SESSION-V1\0");
    hash.update(chain_id.to_be_bytes());
    if local_key < peer_key {
        hash.update(local_key);
        hash.update(local_challenge);
        hash.update(peer_key);
        hash.update(peer_challenge);
    } else {
        hash.update(peer_key);
        hash.update(peer_challenge);
        hash.update(local_key);
        hash.update(local_challenge);
    }
    hash.finalize().into()
}

/// Symmetric challenge-response. Returns peer identity and session ID.
pub fn mutual_handshake(
    stream: &mut TcpStream,
    chain_id: u32,
    local: &SigningKey,
    allowed: &BTreeSet<[u8; 32]>,
    expected: Option<&[u8; 32]>,
) -> Result<AuthenticatedSession, String> {
    let challenge = random_bytes32();
    send_frame(stream, &challenge).map_err(|e| format!("send challenge: {e}"))?;
    let theirs = recv_frame(stream).map_err(|e| format!("receive challenge: {e}"))?;
    let theirs: [u8; 32] = theirs
        .as_slice()
        .try_into()
        .map_err(|_| "Peer challenge must be 32 bytes")?;
    if theirs == [0; 32] || theirs == challenge {
        return Err("Peer sent a zero or reflected challenge".into());
    }
    send_frame(stream, &sign_hello(chain_id, local, &theirs))
        .map_err(|e| format!("send hello: {e}"))?;
    let hello = recv_frame(stream).map_err(|e| format!("receive hello: {e}"))?;
    let verified = verify_hello(&hello, chain_id, &challenge)?;
    let own = local.verifying_key().to_bytes();
    if verified.public_key == own {
        return Err("Peer presented this node's own identity".into());
    }
    if !allowed.contains(&verified.public_key) {
        return Err("Peer key is not a configured validator".into());
    }
    if expected.is_some_and(|key| *key != verified.public_key) {
        return Err("Peer key differs from the configured key for this address".into());
    }
    Ok(AuthenticatedSession {
        peer: verified.public_key,
        session_id: session_id(chain_id, &own, &challenge, &verified.public_key, &theirs),
    })
}

/// Shared transport settings for listener and dialer threads.
pub struct Transport {
    pub chain_id: u32,
    pub key: SigningKey,
    pub allowed: BTreeSet<[u8; 32]>,
    pub events: Sender<Event>,
    pub shutdown: Arc<AtomicBool>,
    pub connected: Arc<Mutex<BTreeSet<[u8; 32]>>>,
}

fn run_connection(
    mut stream: TcpStream,
    transport: Arc<Transport>,
    expected: Option<[u8; 32]>,
    outbound: bool,
) {
    let remote = stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "unknown".into());
    if configure_stream(&stream).is_err() {
        return;
    }
    let session = match mutual_handshake(
        &mut stream,
        transport.chain_id,
        &transport.key,
        &transport.allowed,
        expected.as_ref(),
    ) {
        Ok(session) => session,
        Err(reason) => {
            let _ = stream.shutdown(Shutdown::Both);
            let _ = transport
                .events
                .send(Event::HandshakeRejected { remote, reason });
            return;
        }
    };
    let peer = session.peer;
    let channel = match negotiate(
        &mut stream,
        transport.chain_id,
        session.session_id,
        &transport.key,
        peer,
    ) {
        Ok(channel) => channel,
        Err(reason) => {
            let _ = stream.shutdown(Shutdown::Both);
            let _ = transport
                .events
                .send(Event::HandshakeRejected { remote, reason });
            return;
        }
    };
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    if stream.set_read_timeout(Some(PEER_READ_TIMEOUT)).is_err() {
        return;
    }
    let link = PeerLink {
        key: peer,
        id: NEXT_LINK_ID.fetch_add(1, Ordering::Relaxed),
        outbound,
        remote,
        writer: Arc::new(Mutex::new(SignedWriter {
            stream: writer,
            next_sequence: 1,
        })),
        local_key: transport.key.clone(),
        channel: channel.clone(),
    };
    let link_id = link.id;
    if transport.events.send(Event::PeerUp(link.clone())).is_err() {
        return;
    }
    let mut last_sequence = 0;
    while !transport.shutdown.load(Ordering::Relaxed) {
        match channel.recv(&mut stream, &mut last_sequence) {
            Ok(message) => {
                let inbound = Event::Inbound {
                    peer,
                    link_id,
                    message,
                };
                if transport.events.send(inbound).is_err() {
                    break;
                }
            }
            Err(error) => {
                if error.kind() == io::ErrorKind::InvalidData {
                    let _ = transport.events.send(Event::MalformedMessage {
                        peer,
                        reason: error.to_string(),
                    });
                }
                break;
            }
        }
    }
    link.close();
    let _ = transport.events.send(Event::PeerDown { peer, link_id });
}

/// Accept inbound peers until shutdown. Nonblocking accept lets the
/// listening socket be released promptly for an in-process restart.
pub fn spawn_listener(listener: TcpListener, transport: Arc<Transport>) -> JoinHandle<()> {
    thread::spawn(move || {
        if listener.set_nonblocking(true).is_err() {
            return;
        }
        while !transport.shutdown.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(false).is_ok() {
                        let transport = Arc::clone(&transport);
                        thread::spawn(move || run_connection(stream, transport, None, false));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
    })
}

/// Keep one outbound link to every configured validator whose key is
/// greater than ours; reconnect after failures and restarts.
pub fn spawn_dialer(transport: Arc<Transport>, targets: Vec<([u8; 32], String)>) -> JoinHandle<()> {
    thread::spawn(move || {
        while !transport.shutdown.load(Ordering::Relaxed) {
            for (key, address) in &targets {
                let linked = transport
                    .connected
                    .lock()
                    .map(|set| set.contains(key))
                    .unwrap_or(true);
                if linked {
                    continue;
                }
                let Some(addr) = address
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut addrs| addrs.next())
                else {
                    continue;
                };
                if let Ok(stream) = TcpStream::connect_timeout(&addr, IO_TIMEOUT) {
                    let transport = Arc::clone(&transport);
                    let key = *key;
                    thread::spawn(move || run_connection(stream, transport, Some(key), true));
                    // Give the handshake time to register before redialing.
                    thread::sleep(Duration::from_millis(200));
                }
            }
            thread::sleep(DIAL_INTERVAL);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        configure_stream(&client).unwrap();
        configure_stream(&server).unwrap();
        (client, server)
    }

    fn handshake_both(
        a: SigningKey,
        b: SigningKey,
        allowed: BTreeSet<[u8; 32]>,
    ) -> (
        Result<AuthenticatedSession, String>,
        Result<AuthenticatedSession, String>,
    ) {
        let (mut left, mut right) = pair();
        let allowed_b = allowed.clone();
        let other = thread::spawn(move || mutual_handshake(&mut right, 1, &b, &allowed_b, None));
        let mine = mutual_handshake(&mut left, 1, &a, &allowed, None);
        (mine, other.join().unwrap())
    }

    #[test]
    fn configured_validators_authenticate_each_other() {
        let a = SigningKey::from_bytes(&[1; 32]);
        let b = SigningKey::from_bytes(&[2; 32]);
        let allowed: BTreeSet<_> = [a.verifying_key().to_bytes(), b.verifying_key().to_bytes()]
            .into_iter()
            .collect();
        let (left, right) = handshake_both(a.clone(), b.clone(), allowed);
        let left = left.unwrap();
        let right = right.unwrap();
        assert_eq!(left.peer, b.verifying_key().to_bytes());
        assert_eq!(right.peer, a.verifying_key().to_bytes());
        assert_eq!(left.session_id, right.session_id);
        assert_ne!(left.session_id, [0; 32]);
    }

    #[test]
    fn new_handshake_gets_a_new_session_id() {
        let a = SigningKey::from_bytes(&[1; 32]);
        let b = SigningKey::from_bytes(&[2; 32]);
        let allowed: BTreeSet<_> = [a.verifying_key().to_bytes(), b.verifying_key().to_bytes()]
            .into_iter()
            .collect();
        let (first, _) = handshake_both(a.clone(), b.clone(), allowed.clone());
        let (second, _) = handshake_both(a, b, allowed);
        assert_ne!(first.unwrap().session_id, second.unwrap().session_id);
    }

    #[test]
    fn unknown_peer_is_rejected() {
        let a = SigningKey::from_bytes(&[1; 32]);
        let stranger = SigningKey::from_bytes(&[9; 32]);
        let allowed: BTreeSet<_> = [a.verifying_key().to_bytes()].into_iter().collect();
        let (left, _) = handshake_both(a, stranger, allowed);
        assert!(left.is_err());
    }

    #[test]
    fn expected_key_mismatch_is_rejected() {
        let a = SigningKey::from_bytes(&[1; 32]);
        let b = SigningKey::from_bytes(&[2; 32]);
        let allowed: BTreeSet<_> = [a.verifying_key().to_bytes(), b.verifying_key().to_bytes()]
            .into_iter()
            .collect();
        let (mut left, mut right) = pair();
        let allowed_b = allowed.clone();
        let other = thread::spawn(move || mutual_handshake(&mut right, 1, &b, &allowed_b, None));
        let wrong = [7u8; 32];
        assert!(mutual_handshake(&mut left, 1, &a, &allowed, Some(&wrong)).is_err());
        let _ = other.join();
    }
}
