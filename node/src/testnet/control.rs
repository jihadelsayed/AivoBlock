//! Local operator control API: one framed JSON request per TCP connection,
//! bound to loopback inside the node's container by default. `inject`
//! feeds a message through exactly the same validation as an untrusted
//! peer message and reports the verdict, for adversarial testing.

use std::io;
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use super::node::Event;
use super::wire::WireMessage;
use crate::network::tcp::{configure_stream, recv_frame, send_frame};

const REPLY_TIMEOUT: Duration = Duration::from_secs(30);
/// A local control port can still be reached by compromised same-host
/// processes or an accidentally exposed container port. Bound the number of
/// detached threads waiting on clients or a busy node event queue.
const MAX_CONCURRENT_CONTROL_CONNECTIONS: usize = 16;

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
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let raw = recv_frame(&mut stream)?;
    let answer = match serde_json::from_slice::<ControlRequest>(&raw) {
        Err(error) => json!({ "error": format!("invalid control request: {error}") }),
        Ok(request) => {
            let (reply, receive) = mpsc::channel();
            if events.send(Event::Control { request, reply }).is_err() {
                json!({ "error": "node is shutting down" })
            } else {
                receive
                    .recv_timeout(REPLY_TIMEOUT)
                    .unwrap_or_else(|_| json!({ "error": "node did not answer" }))
            }
        }
    };
    let bytes = serde_json::to_vec(&answer).map_err(io::Error::other)?;
    send_frame(&mut stream, &bytes)
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
