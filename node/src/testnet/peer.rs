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

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};

use super::node::Event;
use super::random_bytes32;
use super::secure::{negotiate, EncryptedChannel};
use super::wire::WireMessage;
use crate::network::handshake::{sign_hello, verify_hello, HELLO_BYTES};
use crate::network::tcp::{configure_stream, recv_frame_bounded, send_frame, IO_TIMEOUT};

/// A live link dies when nothing (not even a heartbeat) arrives for this long.
pub const PEER_READ_TIMEOUT: Duration = Duration::from_secs(15);
const DIAL_INTERVAL: Duration = Duration::from_millis(500);
/// Bound the entire challenge, signed hello and ephemeral-key exchange even
/// when a peer trickles partial frame bytes below the per-read IO timeout.
const PEER_HANDSHAKE_DEADLINE: Duration = Duration::from_secs(10);
/// Cap detached pre-authentication workers so idle/hostile sockets cannot
/// consume unbounded threads. Authenticated links do not occupy these slots.
const MAX_PENDING_INBOUND_HANDSHAKES: usize = 32;
/// Reserve capacity for other origins even if a single address stalls handshakes.
const MAX_PENDING_HANDSHAKES_PER_IP: usize = 8;
/// Limit short-lived connection churn even when every handshake fails quickly.
const MAX_INBOUND_ATTEMPTS_PER_IP_PER_WINDOW: usize = 16;
const INBOUND_ATTEMPT_WINDOW: Duration = Duration::from_secs(1);
/// Bound source-tracking memory under connection floods from many IPs.
const MAX_TRACKED_INBOUND_SOURCES: usize = 256;

static NEXT_LINK_ID: AtomicU64 = AtomicU64::new(1);

/// RAII admission slot, owned by the handshake thread until it either
/// authenticates and negotiates encryption or exits on any failure.
struct HandshakePermit(Arc<AtomicUsize>);

impl HandshakePermit {
    fn acquire(active: &Arc<AtomicUsize>) -> Option<Self> {
        let mut count = active.load(Ordering::Acquire);
        loop {
            if count >= MAX_PENDING_INBOUND_HANDSHAKES {
                return None;
            }
            match active.compare_exchange_weak(
                count,
                count + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(Self(Arc::clone(active))),
                Err(observed) => count = observed,
            }
        }
    }
}

impl Drop for HandshakePermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Count concurrent unauthenticated inbound handshakes per remote IP. A
/// single origin must not starve other configured validators of admission slots.
struct SourcePermit {
    counts: Arc<Mutex<BTreeMap<IpAddr, usize>>>,
    ip: IpAddr,
}

fn canonical_source_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        _ => ip,
    }
}

impl SourcePermit {
    fn acquire(counts: &Arc<Mutex<BTreeMap<IpAddr, usize>>>, ip: IpAddr) -> Option<Self> {
        let ip = canonical_source_ip(ip);
        let mut locked = counts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = locked.entry(ip).or_default();
        if *count >= MAX_PENDING_HANDSHAKES_PER_IP {
            return None;
        }
        *count += 1;
        Some(Self {
            counts: Arc::clone(counts),
            ip,
        })
    }
}

impl Drop for SourcePermit {
    fn drop(&mut self) {
        let mut locked = self
            .counts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = locked.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                locked.remove(&self.ip);
            }
        }
    }
}

/// Reserving the global slot first ensures the source map never grows beyond
/// the global admission bound. A rejected per-IP reservation automatically
/// releases the global slot through the RAII guard.
struct InboundAdmission {
    _global: HandshakePermit,
    _source: SourcePermit,
}

impl InboundAdmission {
    fn acquire(
        pending: &Arc<AtomicUsize>,
        per_ip: &Arc<Mutex<BTreeMap<IpAddr, usize>>>,
        ip: IpAddr,
    ) -> Option<Self> {
        let global = HandshakePermit::acquire(pending)?;
        let source = SourcePermit::acquire(per_ip, ip)?;
        Some(Self {
            _global: global,
            _source: source,
        })
    }
}

/// Single-threaded listener admission meter. Count even failed/rejected
/// attempts so rapid reconnect churn cannot repeatedly start handshake work.
#[derive(Clone, Copy)]
struct SourceAttemptWindow {
    started: Instant,
    attempts: usize,
}

#[derive(Default)]
struct SourceRateLimiter {
    sources: BTreeMap<IpAddr, SourceAttemptWindow>,
}

impl SourceRateLimiter {
    fn allow(&mut self, source: IpAddr, now: Instant) -> bool {
        let source = canonical_source_ip(source);
        if let Some(window) = self.sources.get_mut(&source) {
            if now.duration_since(window.started) >= INBOUND_ATTEMPT_WINDOW {
                *window = SourceAttemptWindow {
                    started: now,
                    attempts: 1,
                };
                return true;
            }
            if window.attempts >= MAX_INBOUND_ATTEMPTS_PER_IP_PER_WINDOW {
                return false;
            }
            window.attempts += 1;
            return true;
        }

        // Retire expired windows when full, but never grow the map past its
        // fixed bound. Unknown sources are rejected while every slot is fresh.
        if self.sources.len() >= MAX_TRACKED_INBOUND_SOURCES {
            self.sources
                .retain(|_, window| now.duration_since(window.started) < INBOUND_ATTEMPT_WINDOW);
            if self.sources.len() >= MAX_TRACKED_INBOUND_SOURCES {
                return false;
            }
        }
        self.sources.insert(
            source,
            SourceAttemptWindow {
                started: now,
                attempts: 1,
            },
        );
        true
    }
}

/// Hold an outbound target reservation for the full lifetime of its connection
/// worker. An unauthenticated or stalled target must not cause the dialer to
/// launch duplicate workers every reconnect tick before PeerUp is processed.
struct OutboundAttemptPermit {
    dialing: Arc<Mutex<BTreeSet<[u8; 32]>>>,
    peer: [u8; 32],
}

impl OutboundAttemptPermit {
    fn acquire(
        dialing: &Arc<Mutex<BTreeSet<[u8; 32]>>>,
        peer: [u8; 32],
    ) -> Option<Self> {
        let mut active = dialing.lock().ok()?;
        if !active.insert(peer) {
            return None;
        }
        Some(Self {
            dialing: Arc::clone(dialing),
            peer,
        })
    }
}

impl Drop for OutboundAttemptPermit {
    fn drop(&mut self) {
        let mut active = self
            .dialing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active.remove(&self.peer);
    }
}

/// TCP reader/writer with a single absolute handshake deadline. A socket
/// read timeout is per blocking operation and can otherwise be repeatedly
/// extended by a peer that sends only one byte at a time.
struct DeadlineStream<'a> {
    inner: &'a mut TcpStream,
    deadline: Instant,
}

impl<'a> DeadlineStream<'a> {
    fn new(inner: &'a mut TcpStream, timeout: Duration) -> Self {
        Self {
            inner,
            deadline: Instant::now() + timeout,
        }
    }

    fn remaining(&self) -> io::Result<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Peer handshake deadline exceeded",
            ));
        }
        Ok(remaining.min(IO_TIMEOUT))
    }
}

impl Read for DeadlineStream<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let timeout = self.remaining()?;
        self.inner.set_read_timeout(Some(timeout))?;
        self.inner.read(buf)
    }
}

impl Write for DeadlineStream<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let timeout = self.remaining()?;
        self.inner.set_write_timeout(Some(timeout))?;
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.remaining()?;
        self.inner.flush()
    }
}

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
pub fn mutual_handshake<S: Read + Write>(
    stream: &mut S,
    chain_id: u32,
    local: &SigningKey,
    allowed: &BTreeSet<[u8; 32]>,
    expected: Option<&[u8; 32]>,
) -> Result<AuthenticatedSession, String> {
    let challenge = random_bytes32();
    send_frame(stream, &challenge).map_err(|e| format!("send challenge: {e}"))?;
    let theirs = recv_frame_bounded(stream, 32).map_err(|e| format!("receive challenge: {e}"))?;
    let theirs: [u8; 32] = theirs
        .as_slice()
        .try_into()
        .map_err(|_| "Peer challenge must be 32 bytes")?;
    if theirs == [0; 32] || theirs == challenge {
        return Err("Peer sent a zero or reflected challenge".into());
    }
    send_frame(stream, &sign_hello(chain_id, local, &theirs))
        .map_err(|e| format!("send hello: {e}"))?;
    let hello =
        recv_frame_bounded(stream, HELLO_BYTES).map_err(|e| format!("receive hello: {e}"))?;
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
    admission: Option<InboundAdmission>,
) {
    let remote = stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "unknown".into());
    if configure_stream(&stream).is_err() {
        return;
    }
    // A single absolute budget covers challenge-response and signed X25519,
    // including partial frame reads and writes.
    let handshake = (|| {
        let mut timed = DeadlineStream::new(&mut stream, PEER_HANDSHAKE_DEADLINE);
        let session = mutual_handshake(
            &mut timed,
            transport.chain_id,
            &transport.key,
            &transport.allowed,
            expected.as_ref(),
        )?;
        let channel = negotiate(
            &mut timed,
            transport.chain_id,
            session.session_id,
            &transport.key,
            session.peer,
        )?;
        Ok::<_, String>((session.peer, channel))
    })();
    let (peer, channel) = match handshake {
        Ok(result) => result,
        Err(reason) => {
            let _ = stream.shutdown(Shutdown::Both);
            let _ = transport
                .events
                .send(Event::HandshakeRejected { remote, reason });
            return;
        }
    };
    // Once both authentication stages pass, free capacity for new peers.
    // Every early return above also releases its slot through Drop.
    drop(admission);
    // A successful deadline-limited write may leave a tiny socket timeout.
    // Restore normal link write timeouts before exchanging application frames.
    if stream.set_write_timeout(Some(IO_TIMEOUT)).is_err() {
        return;
    }
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
    if transport
        .events
        .send(Event::PeerUp(Box::new(link.clone())))
        .is_err()
    {
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
        let pending = Arc::new(AtomicUsize::new(0));
        let per_ip = Arc::new(Mutex::new(BTreeMap::new()));
        let mut rate_limiter = SourceRateLimiter::default();
        if listener.set_nonblocking(true).is_err() {
            return;
        }
        while !transport.shutdown.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, address)) => {
                    if !rate_limiter.allow(address.ip(), Instant::now()) {
                        // Reject rate-limited sockets before creating a worker.
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    }
                    if stream.set_nonblocking(false).is_ok() {
                        let Some(permit) =
                            InboundAdmission::acquire(&pending, &per_ip, address.ip())
                        else {
                            // Never start a worker for an over-capacity socket;
                            // avoid events that would amplify admission floods.
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        };
                        let transport = Arc::clone(&transport);
                        thread::spawn(move || {
                            run_connection(stream, transport, None, false, Some(permit))
                        });
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
        let dialing = Arc::new(Mutex::new(BTreeSet::new()));
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
                // Reserve before DNS and TCP connect, so a pending handshake
                // cannot be redialed until its worker actually exits.
                let Some(attempt) = OutboundAttemptPermit::acquire(&dialing, *key) else {
                    continue;
                };
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
                    thread::spawn(move || {
                        let _attempt = attempt;
                        run_connection(stream, transport, Some(key), true, None);
                    });
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
    fn inbound_handshake_capacity_rejects_overflow_and_releases_slots() {
        let active = Arc::new(AtomicUsize::new(0));
        let permits: Vec<_> = (0..MAX_PENDING_INBOUND_HANDSHAKES)
            .map(|_| HandshakePermit::acquire(&active).unwrap())
            .collect();
        assert_eq!(
            active.load(Ordering::Acquire),
            MAX_PENDING_INBOUND_HANDSHAKES
        );
        assert!(HandshakePermit::acquire(&active).is_none());
        drop(permits);
        assert_eq!(active.load(Ordering::Acquire), 0);
        let recovered = HandshakePermit::acquire(&active).unwrap();
        assert_eq!(active.load(Ordering::Acquire), 1);
        drop(recovered);
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn simultaneous_admission_cannot_exceed_handshake_capacity() {
        use std::sync::Barrier;

        let workers = MAX_PENDING_INBOUND_HANDSHAKES * 2;
        let active = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(workers + 1));
        let attempted = Arc::new(Barrier::new(workers + 1));
        let release = Arc::new(Barrier::new(workers + 1));
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let active = Arc::clone(&active);
                let start = Arc::clone(&start);
                let attempted = Arc::clone(&attempted);
                let release = Arc::clone(&release);
                thread::spawn(move || {
                    start.wait();
                    let permit = HandshakePermit::acquire(&active);
                    attempted.wait();
                    release.wait();
                    permit.is_some()
                })
            })
            .collect();
        start.wait();
        attempted.wait();
        let held = active.load(Ordering::Acquire);
        release.wait();
        assert_eq!(held, MAX_PENDING_INBOUND_HANDSHAKES);
        assert_eq!(
            handles
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .filter(|accepted| *accepted)
                .count(),
            MAX_PENDING_INBOUND_HANDSHAKES
        );
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn per_ip_capacity_preserves_slots_for_other_origins_and_recovers() {
        let pending = Arc::new(AtomicUsize::new(0));
        let per_ip = Arc::new(Mutex::new(BTreeMap::new()));
        let source_a: IpAddr = "192.0.2.1".parse().unwrap();
        let source_b: IpAddr = "192.0.2.2".parse().unwrap();
        let permits: Vec<_> = (0..MAX_PENDING_HANDSHAKES_PER_IP)
            .map(|_| InboundAdmission::acquire(&pending, &per_ip, source_a).unwrap())
            .collect();
        assert!(InboundAdmission::acquire(&pending, &per_ip, source_a).is_none());
        // A rejected source must not leak its temporary global admission slot.
        assert_eq!(
            pending.load(Ordering::Acquire),
            MAX_PENDING_HANDSHAKES_PER_IP
        );
        let other = InboundAdmission::acquire(&pending, &per_ip, source_b).unwrap();
        assert_eq!(
            pending.load(Ordering::Acquire),
            MAX_PENDING_HANDSHAKES_PER_IP + 1
        );
        drop(other);
        drop(permits);
        assert_eq!(pending.load(Ordering::Acquire), 0);
        assert!(per_ip.lock().unwrap().is_empty());
        let retry = InboundAdmission::acquire(&pending, &per_ip, source_a).unwrap();
        drop(retry);
        assert!(per_ip.lock().unwrap().is_empty());
    }

    #[test]
    fn mapped_ipv4_source_shares_admission_capacity_with_ipv4() {
        let pending = Arc::new(AtomicUsize::new(0));
        let per_ip = Arc::new(Mutex::new(BTreeMap::new()));
        let direct: IpAddr = "192.0.2.7".parse().unwrap();
        let mapped: IpAddr = "::ffff:192.0.2.7".parse().unwrap();
        assert_eq!(canonical_source_ip(mapped), direct);
        let permits: Vec<_> = (0..MAX_PENDING_HANDSHAKES_PER_IP)
            .map(|_| InboundAdmission::acquire(&pending, &per_ip, direct).unwrap())
            .collect();
        assert!(InboundAdmission::acquire(&pending, &per_ip, mapped).is_none());
        drop(permits);
        assert!(per_ip.lock().unwrap().is_empty());
    }

    #[test]
    fn concurrent_same_source_admissions_cannot_exceed_per_ip_cap() {
        use std::sync::Barrier;

        let workers = MAX_PENDING_HANDSHAKES_PER_IP * 2;
        let pending = Arc::new(AtomicUsize::new(0));
        let per_ip = Arc::new(Mutex::new(BTreeMap::new()));
        let source: IpAddr = "192.0.2.9".parse().unwrap();
        let start = Arc::new(Barrier::new(workers + 1));
        let attempted = Arc::new(Barrier::new(workers + 1));
        let release = Arc::new(Barrier::new(workers + 1));
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let pending = Arc::clone(&pending);
                let per_ip = Arc::clone(&per_ip);
                let start = Arc::clone(&start);
                let attempted = Arc::clone(&attempted);
                let release = Arc::clone(&release);
                thread::spawn(move || {
                    start.wait();
                    let permit = InboundAdmission::acquire(&pending, &per_ip, source);
                    attempted.wait();
                    release.wait();
                    permit.is_some()
                })
            })
            .collect();
        start.wait();
        attempted.wait();
        assert_eq!(
            pending.load(Ordering::Acquire),
            MAX_PENDING_HANDSHAKES_PER_IP
        );
        release.wait();
        assert_eq!(
            handles
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .filter(|accepted| *accepted)
                .count(),
            MAX_PENDING_HANDSHAKES_PER_IP
        );
        assert_eq!(pending.load(Ordering::Acquire), 0);
        assert!(per_ip.lock().unwrap().is_empty());
    }

    #[test]
    fn per_ip_rate_limit_rejects_rapid_churn_and_recovers_after_window() {
        let mut limiter = SourceRateLimiter::default();
        let source: IpAddr = "192.0.2.25".parse().unwrap();
        let now = Instant::now();
        for _ in 0..MAX_INBOUND_ATTEMPTS_PER_IP_PER_WINDOW {
            assert!(limiter.allow(source, now));
        }
        assert!(!limiter.allow(source, now));
        assert!(!limiter.allow(
            source,
            now + INBOUND_ATTEMPT_WINDOW - Duration::from_nanos(1)
        ));
        let next = now + INBOUND_ATTEMPT_WINDOW;
        assert!(limiter.allow(source, next));
        assert_eq!(limiter.sources.get(&source).unwrap().attempts, 1);
    }

    #[test]
    fn per_ip_rate_limit_isolated_and_ipv4_mapped_addresses_share_counter() {
        let mut limiter = SourceRateLimiter::default();
        let first: IpAddr = "192.0.2.26".parse().unwrap();
        let mapped: IpAddr = "::ffff:192.0.2.26".parse().unwrap();
        let second: IpAddr = "192.0.2.27".parse().unwrap();
        let now = Instant::now();
        for _ in 0..MAX_INBOUND_ATTEMPTS_PER_IP_PER_WINDOW {
            assert!(limiter.allow(first, now));
        }
        assert!(!limiter.allow(mapped, now));
        assert!(limiter.allow(second, now));
        assert_eq!(limiter.sources.len(), 2);
    }

    #[test]
    fn rate_limit_source_tracking_is_bounded_and_reclaims_expired_entries() {
        use std::net::Ipv4Addr;

        let mut limiter = SourceRateLimiter::default();
        let now = Instant::now();
        for n in 0..MAX_TRACKED_INBOUND_SOURCES {
            let source = IpAddr::V4(Ipv4Addr::new(192, 0, (n / 256) as u8, (n % 256) as u8));
            assert!(limiter.allow(source, now));
        }
        let new_source: IpAddr = "203.0.113.30".parse().unwrap();
        assert_eq!(limiter.sources.len(), MAX_TRACKED_INBOUND_SOURCES);
        assert!(!limiter.allow(new_source, now));
        assert_eq!(limiter.sources.len(), MAX_TRACKED_INBOUND_SOURCES);
        let later = now + INBOUND_ATTEMPT_WINDOW;
        assert!(limiter.allow(new_source, later));
        assert_eq!(limiter.sources.len(), 1);
    }

    #[test]
    fn outbound_attempts_are_unique_per_validator_and_release_on_failure() {
        let dialing = Arc::new(Mutex::new(BTreeSet::new()));
        let first = [1u8; 32];
        let second = [2u8; 32];

        let attempt = OutboundAttemptPermit::acquire(&dialing, first).unwrap();
        assert!(OutboundAttemptPermit::acquire(&dialing, first).is_none());
        let other = OutboundAttemptPermit::acquire(&dialing, second).unwrap();
        assert_eq!(dialing.lock().unwrap().len(), 2);

        drop(attempt);
        assert!(!dialing.lock().unwrap().contains(&first));
        let retried = OutboundAttemptPermit::acquire(&dialing, first).unwrap();
        assert!(OutboundAttemptPermit::acquire(&dialing, first).is_none());
        drop(retried);
        drop(other);
        assert!(dialing.lock().unwrap().is_empty());
    }

    #[test]
    fn concurrent_outbound_reservations_allow_only_one_worker_for_a_target() {
        use std::sync::Barrier;

        let dialing = Arc::new(Mutex::new(BTreeSet::new()));
        let start = Arc::new(Barrier::new(17));
        let hold = Arc::new(Barrier::new(17));
        let release = Arc::new(Barrier::new(17));
        let workers: Vec<_> = (0..16)
            .map(|_| {
                let dialing = Arc::clone(&dialing);
                let start = Arc::clone(&start);
                let hold = Arc::clone(&hold);
                let release = Arc::clone(&release);
                thread::spawn(move || {
                    start.wait();
                    let permit = OutboundAttemptPermit::acquire(&dialing, [7u8; 32]);
                    hold.wait();
                    release.wait();
                    permit.is_some()
                })
            })
            .collect();
        start.wait();
        hold.wait();
        assert_eq!(dialing.lock().unwrap().len(), 1);
        release.wait();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .filter(|accepted| *accepted)
                .count(),
            1
        );
        assert!(dialing.lock().unwrap().is_empty());
    }

    #[test]
    fn peer_handshake_deadline_does_not_reset_when_bytes_arrive() {
        let (mut client, mut server) = pair();
        let mut timed = DeadlineStream::new(&mut client, Duration::from_secs(3));
        server.write_all(b"ab").unwrap();
        let mut byte = [0u8; 1];
        assert_eq!(timed.read(&mut byte).unwrap(), 1);
        assert_eq!(byte, [b'a']);

        // A successful partial read must not give the peer another timeout.
        timed.deadline = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            timed.read(&mut byte).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            timed.write(b"x").unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(timed.flush().unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn valid_peers_complete_both_handshake_stages_with_one_deadline() {
        let local = SigningKey::from_bytes(&[1; 32]);
        let remote = SigningKey::from_bytes(&[2; 32]);
        let allowed: BTreeSet<_> = [
            local.verifying_key().to_bytes(),
            remote.verifying_key().to_bytes(),
        ]
        .into_iter()
        .collect();
        let (mut client, mut server) = pair();
        let allowed_remote = allowed.clone();
        let server_thread = thread::spawn(move || {
            let mut timed = DeadlineStream::new(&mut server, Duration::from_secs(3));
            let session = mutual_handshake(&mut timed, 1, &remote, &allowed_remote, None).unwrap();
            let channel =
                negotiate(&mut timed, 1, session.session_id, &remote, session.peer).unwrap();
            (session.session_id, channel)
        });
        let mut timed = DeadlineStream::new(&mut client, Duration::from_secs(3));
        let session = mutual_handshake(&mut timed, 1, &local, &allowed, None).unwrap();
        let channel = negotiate(&mut timed, 1, session.session_id, &local, session.peer).unwrap();
        let (remote_session, remote_channel) = server_thread.join().unwrap();
        assert_eq!(session.session_id, remote_session);
        let message = WireMessage::Status {
            height: 1,
            tip: "00".into(),
        };
        let mut encrypted = Vec::new();
        channel.send(&mut encrypted, &message, &local, 1).unwrap();
        let received = remote_channel
            .recv(&mut io::Cursor::new(encrypted), &mut 0)
            .unwrap();
        assert_eq!(received, message);
    }

    #[test]
    fn oversized_challenge_is_rejected_before_its_body_is_read() {
        use std::io::Write;

        let local = SigningKey::from_bytes(&[1; 32]);
        let allowed: BTreeSet<_> = [SigningKey::from_bytes(&[2; 32]).verifying_key().to_bytes()]
            .into_iter()
            .collect();
        let (mut client, mut server) = pair();
        server.write_all(&33u32.to_be_bytes()).unwrap();
        let err = mutual_handshake(&mut client, 1, &local, &allowed, None).unwrap_err();
        assert!(err.contains("receive challenge"));
    }

    #[test]
    fn oversized_hello_is_rejected_before_its_body_is_read() {
        use std::io::Write;

        let local = SigningKey::from_bytes(&[1; 32]);
        let remote = SigningKey::from_bytes(&[2; 32]);
        let allowed: BTreeSet<_> = [remote.verifying_key().to_bytes()].into_iter().collect();
        let (mut client, mut server) = pair();
        let peer_thread = thread::spawn(move || {
            // Respond with a legal challenge, read the local signed hello,
            // then advertise an oversized peer hello without sending its body.
            assert_eq!(recv_frame_bounded(&mut server, 32).unwrap().len(), 32);
            send_frame(&mut server, &[8u8; 32]).unwrap();
            assert_eq!(
                recv_frame_bounded(&mut server, HELLO_BYTES).unwrap().len(),
                HELLO_BYTES
            );
            server
                .write_all(&((HELLO_BYTES + 1) as u32).to_be_bytes())
                .unwrap();
        });
        let err = mutual_handshake(&mut client, 1, &local, &allowed, None).unwrap_err();
        assert!(err.contains("receive hello"));
        peer_thread.join().unwrap();
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
