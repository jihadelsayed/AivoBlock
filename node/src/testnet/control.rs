//! Local operator control API: one framed JSON request per TCP connection,
//! bound to loopback inside the node's container by default. `inject`
//! feeds a message through exactly the same validation as an untrusted
//! peer message and reports the verdict, for adversarial testing.

use std::io;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
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
        if listener.set_nonblocking(true).is_err() {
            return;
        }
        while !shutdown.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(false).is_ok() {
                        let events = events.clone();
                        thread::spawn(move || {
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
