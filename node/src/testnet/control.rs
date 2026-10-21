//! Local operator control API: one framed JSON request per TCP connection,
//! bound to loopback inside the node's container by default. `inject`
//! feeds a message through exactly the same validation as an untrusted
//! peer message and reports the verdict, for adversarial testing.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use super::node::Event;
use super::wire::WireMessage;
use crate::network::tcp::{configure_stream, recv_frame, send_frame, IO_TIMEOUT};

const REPLY_TIMEOUT: Duration = Duration::from_secs(30);
/// Bound each complete request and response frame, not just each socket read
/// or write. Slow peers otherwise keep a connection slot by trickling bytes.
const CONTROL_FRAME_DEADLINE: Duration = Duration::from_secs(10);
/// A local control port can still be reached by compromised same-host
/// processes or an accidentally exposed container port. Bound the number of
/// detached threads waiting on clients or a busy node event queue.
const MAX_CONCURRENT_CONTROL_CONNECTIONS: usize = 16;

/// Operator control commands are intentionally unauthenticated and may inject
/// synthetic consensus messages. Refuse wildcard/public binds rather than
/// accidentally exposing these privileged operations beyond the local host.
pub(super) fn require_local_control_listener(listener: &TcpListener) -> Result<(), String> {
    let address = listener
        .local_addr()
        .map_err(|error| format!("inspect operator control listener: {error}"))?;
    if !address.ip().is_loopback() {
        return Err(format!(
            "Operator control listener must bind a loopback address, not {address}"
        ));
    }
    Ok(())
}

/// TCP stream adapter enforcing one absolute deadline per control frame.
/// TCP read/write timeouts apply to individual syscalls and do not stop
/// trickle traffic unless the remaining end-to-end time is checked each time.
struct ControlDeadlineStream<'a> {
    inner: &'a mut TcpStream,
    deadline: Instant,
}

impl<'a> ControlDeadlineStream<'a> {
    fn new(inner: &'a mut TcpStream, duration: Duration) -> Self {
        Self {
            inner,
            deadline: Instant::now() + duration,
        }
    }

    fn remaining(&self) -> io::Result<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Control frame deadline exceeded",
            ));
        }
        Ok(remaining.min(IO_TIMEOUT))
    }
}

impl Read for ControlDeadlineStream<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let timeout = self.remaining()?;
        self.inner.set_read_timeout(Some(timeout))?;
        self.inner.read(buf)
    }
}

impl Write for ControlDeadlineStream<'_> {
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

/// A reservation is held until the control handler exits, including all
/// timeout and parsing-error paths.
struct ControlConnectionPermit(Arc<AtomicUsize>);

impl ControlConnectionPermit {
    fn acquire(active: &Arc<AtomicUsize>) -> Option<Self> {
        let mut count = active.load(Ordering::Acquire);
        loop {
            if count >= MAX_CONCURRENT_CONTROL_CONNECTIONS {
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

impl Drop for ControlConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Clone, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlRequest {
    Status,
    SubmitTask { task: String },
    Inject { message: WireMessage },
    GetBlock { height: u64 },
}

fn serve_one(mut stream: TcpStream, events: &SyncSender<Event>) -> io::Result<()> {
    configure_stream(&stream)?;
    // A partial header or payload cannot repeatedly renew the read timeout.
    let raw = {
        let mut request = ControlDeadlineStream::new(&mut stream, CONTROL_FRAME_DEADLINE);
        recv_frame(&mut request)?
    };
    let answer = match serde_json::from_slice::<ControlRequest>(&raw) {
        Err(error) => json!({ "error": format!("invalid control request: {error}") }),
        Ok(request) => {
            let (reply, receive) = mpsc::channel();
            // The node event channel is bounded. Blocking on send() here can
            // strand all 16 operator handler slots indefinitely when the node
            // is overloaded, bypassing the per-frame and reply deadlines.
            match events.try_send(Event::Control { request, reply }) {
                Ok(()) => receive
                    .recv_timeout(REPLY_TIMEOUT)
                    .unwrap_or_else(|_| json!({ "error": "node did not answer" })),
                Err(mpsc::TrySendError::Full(_)) => {
                    json!({ "error": "node control event queue is full" })
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    json!({ "error": "node is shutting down" })
                }
            }
        }
    };
    let bytes = serde_json::to_vec(&answer).map_err(io::Error::other)?;
    // Replies use their own deadline, after the node has produced its answer.
    let mut response = ControlDeadlineStream::new(&mut stream, CONTROL_FRAME_DEADLINE);
    send_frame(&mut response, &bytes)
}

pub fn spawn_control_server(
    listener: TcpListener,
    events: SyncSender<Event>,
    shutdown: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let active = Arc::new(AtomicUsize::new(0));
        if listener.set_nonblocking(true).is_err() {
            return;
        }
        while !shutdown.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(false).is_ok() {
                        let Some(permit) = ControlConnectionPermit::acquire(&active) else {
                            // Refuse excess sockets without spawning a thread.
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        };
                        let events = events.clone();
                        thread::spawn(move || {
                            let _permit = permit;
                            let _ = serve_one(stream, &events);
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

/// Client side: send one request and return the node's JSON answer.
pub fn control_call(address: &str, request: &ControlRequest) -> Result<Value, String> {
    let addr = address
        .to_socket_addrs()
        .map_err(|e| format!("{address}: {e}"))?
        .next()
        .ok_or_else(|| format!("{address}: no address"))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| format!("connect {address}: {e}"))?;
    stream
        .set_read_timeout(Some(REPLY_TIMEOUT + Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    send_frame(&mut stream, &bytes).map_err(|e| format!("send: {e}"))?;
    let reply = recv_frame(&mut stream).map_err(|e| format!("receive: {e}"))?;
    serde_json::from_slice(&reply).map_err(|e| format!("invalid reply: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_control_rejects_wildcard_binds_and_accepts_loopback() {
        let local = TcpListener::bind("127.0.0.1:0").unwrap();
        require_local_control_listener(&local).unwrap();

        // A wildcard listener is reachable from non-loopback interfaces,
        // even if clients usually connect over 127.0.0.1.
        let exposed = TcpListener::bind("0.0.0.0:0").unwrap();
        let failure = require_local_control_listener(&exposed).unwrap_err();
        assert!(failure.contains("must bind a loopback address"));
        assert!(failure.contains("0.0.0.0:"));

        // IPv6 wildcard binds must be rejected as well where IPv6 works.
        if let Ok(exposed_v6) = TcpListener::bind("[::]:0") {
            assert!(require_local_control_listener(&exposed_v6).is_err());
        }
        if let Ok(local_v6) = TcpListener::bind("[::1]:0") {
            require_local_control_listener(&local_v6).unwrap();
        }
    }

    #[test]
    fn control_frame_deadline_survives_partial_reads_and_limits_writes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let mut limited = ControlDeadlineStream::new(&mut server, Duration::from_secs(3));
        client.write_all(b"ab").unwrap();
        let mut byte = [0u8; 1];
        assert_eq!(limited.read(&mut byte).unwrap(), 1);
        assert_eq!(byte, [b'a']);

        // Reading a byte cannot restart the absolute timer.
        limited.deadline = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            limited.read(&mut byte).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            limited.write(b"x").unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(limited.flush().unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn complete_control_status_request_and_reply_use_deadline_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        configure_stream(&client).unwrap();
        let (events, received) = mpsc::sync_channel(1);
        let handler = thread::spawn(move || serve_one(server, &events).unwrap());

        send_frame(
            &mut client,
            &serde_json::to_vec(&ControlRequest::Status).unwrap(),
        )
        .unwrap();
        match received.recv_timeout(Duration::from_secs(3)).unwrap() {
            Event::Control {
                request: ControlRequest::Status,
                reply,
            } => {
                reply.send(json!({"status": "ok"})).unwrap();
            }
            _ => panic!("Expected status control request"),
        }
        let response = recv_frame(&mut client).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&response).unwrap(),
            json!({"status": "ok"})
        );
        handler.join().unwrap();
    }

    #[test]
    fn full_node_event_queue_returns_error_without_blocking_control_handler() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        configure_stream(&client).unwrap();

        // Occupy the entire bounded queue without starting the node loop.
        // The second request must return an error instead of blocking send().
        let (events, receiver) = mpsc::sync_channel(1);
        let (first_reply, _) = mpsc::channel();
        assert!(events
            .try_send(Event::Control {
                request: ControlRequest::Status,
                reply: first_reply,
            })
            .is_ok());
        let handler = thread::spawn(move || serve_one(server, &events).unwrap());

        send_frame(
            &mut client,
            &serde_json::to_vec(&ControlRequest::Status).unwrap(),
        )
        .unwrap();
        let response = recv_frame(&mut client).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&response).unwrap(),
            json!({"error": "node control event queue is full"})
        );
        handler.join().unwrap();
        assert!(matches!(
            receiver.try_recv(),
            Ok(Event::Control {
                request: ControlRequest::Status,
                ..
            })
        ));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn disconnected_node_event_queue_returns_shutdown_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        configure_stream(&client).unwrap();

        let (events, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        let handler = thread::spawn(move || serve_one(server, &events).unwrap());
        send_frame(
            &mut client,
            &serde_json::to_vec(&ControlRequest::Status).unwrap(),
        )
        .unwrap();
        let reply = recv_frame(&mut client).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&reply).unwrap(),
            json!({"error": "node is shutting down"})
        );
        handler.join().unwrap();
    }

    #[test]
    fn control_connection_capacity_is_bounded_and_released() {
        let active = Arc::new(AtomicUsize::new(0));
        let permits: Vec<_> = (0..MAX_CONCURRENT_CONTROL_CONNECTIONS)
            .map(|_| ControlConnectionPermit::acquire(&active).unwrap())
            .collect();
        assert_eq!(
            active.load(Ordering::Acquire),
            MAX_CONCURRENT_CONTROL_CONNECTIONS
        );
        assert!(ControlConnectionPermit::acquire(&active).is_none());
        drop(permits);
        assert_eq!(active.load(Ordering::Acquire), 0);
        let recovered = ControlConnectionPermit::acquire(&active).unwrap();
        drop(recovered);
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn concurrent_control_admission_cannot_exceed_capacity() {
        use std::sync::Barrier;

        let workers = MAX_CONCURRENT_CONTROL_CONNECTIONS * 2;
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
                    let permit = ControlConnectionPermit::acquire(&active);
                    attempted.wait();
                    release.wait();
                    permit.is_some()
                })
            })
            .collect();
        start.wait();
        attempted.wait();
        assert_eq!(
            active.load(Ordering::Acquire),
            MAX_CONCURRENT_CONTROL_CONNECTIONS
        );
        release.wait();
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|accepted| *accepted)
                .count(),
            MAX_CONCURRENT_CONTROL_CONNECTIONS
        );
        assert_eq!(active.load(Ordering::Acquire), 0);
    }
}
