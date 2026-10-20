//! Bounded TCP framing for a future authenticated node transport.
//! A 4-byte big-endian length prefix precedes each nonempty payload.
//! This does not authenticate peers, encrypt traffic, or establish consensus.
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::MAX_MESSAGE_BYTES;

pub const IO_TIMEOUT: Duration = Duration::from_secs(3);

pub fn configure_stream(stream: &TcpStream) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    stream.set_nodelay(true)
}

pub fn send_frame<W: Write>(stream: &mut W, payload: &[u8]) -> io::Result<()> {
    if payload.is_empty() || payload.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid frame size"));
    }
    let size = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Frame too large"))?;
    stream.write_all(&size.to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}

pub fn recv_frame<R: Read>(stream: &mut R) -> io::Result<Vec<u8>> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid frame length"));
    }
    let mut payload = vec![0u8; length];
    stream.read_exact(&mut payload)?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn bounded_frames_round_trip() {
        let mut output = Vec::new();
        send_frame(&mut output, b"hello").unwrap();
        assert_eq!(recv_frame(&mut Cursor::new(output)).unwrap(), b"hello");
    }

    #[test]
    fn rejects_empty_oversize_and_truncated_frames() {
        assert!(send_frame(&mut Vec::new(), b"").is_err());
        assert!(send_frame(&mut Vec::new(), &vec![0; MAX_MESSAGE_BYTES + 1]).is_err());
        for frame in [
            Vec::new(),
            0u32.to_be_bytes().to_vec(),
            ((MAX_MESSAGE_BYTES as u32) + 1).to_be_bytes().to_vec(),
            vec![0, 0, 0, 3, 1],
        ] {
            assert!(recv_frame(&mut Cursor::new(frame)).is_err());
        }
    }

    #[test]
    fn two_local_nodes_exchange_framed_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            configure_stream(&stream)?;
            assert_eq!(recv_frame(&mut stream)?, b"ping from node-2");
            send_frame(&mut stream, b"pong from node-1")?;
            Ok(())
        });
        let mut client = TcpStream::connect_timeout(&addr, IO_TIMEOUT).unwrap();
        configure_stream(&client).unwrap();
        send_frame(&mut client, b"ping from node-2").unwrap();
        assert_eq!(recv_frame(&mut client).unwrap(), b"pong from node-1");
        server.join().unwrap().unwrap();
    }
}
