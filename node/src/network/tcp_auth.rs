//! Single-direction TCP peer authentication for loopback research tests.
//! The server must supply a fresh, unpredictable, one-use challenge from a
//! trusted caller. This module does not generate challenges, track expiry,
//! authenticate the server to the client, or encrypt the connection.
use std::io;
use std::net::TcpStream;

use ed25519_dalek::{Signer, SigningKey};

use super::handshake::{verify_hello, VerifiedHello, HELLO_BYTES, HELLO_VERSION};
use super::tcp::{recv_frame, send_frame};

const DOMAIN: &[u8] = b"AIVOBLOCK-PEER-HELLO-V1\0";

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Server challenge and signed response. The caller must generate and consume
/// challenge exactly once and must verify authorized identity separately.
/// The socket is NOT a confidential or mutually authenticated channel.
pub fn accept_authenticated_hello(
    stream: &mut TcpStream,
    chain_id: u32,
    challenge: [u8; 32],
    expected_peer_key: &[u8; 32],
) -> io::Result<VerifiedHello> {
    if chain_id == 0 || challenge == [0; 32] {
        return Err(invalid("Invalid server handshake configuration"));
    }
    send_frame(stream, &challenge)?;
    let response = recv_frame(stream)?;
    let verified = verify_hello(&response, chain_id, &challenge).map_err(invalid)?;
    if &verified.public_key != expected_peer_key {
        return Err(invalid("Peer identity is not authorized"));
    }
    Ok(verified)
}

/// Send a signed hello in response to a server challenge. This authenticates
/// the client to the server, not the server to the client.
pub fn respond_to_challenge(
    stream: &mut TcpStream,
    chain_id: u32,
    local_key: &SigningKey,
) -> io::Result<()> {
    if chain_id == 0 {
        return Err(invalid("Chain ID must be nonzero"));
    }
    let challenge_frame = recv_frame(stream)?;
    if challenge_frame.len() != 32 || challenge_frame.iter().all(|byte| *byte == 0) {
        return Err(invalid("Invalid peer challenge"));
    }
    let mut message = Vec::with_capacity(HELLO_BYTES);
    message.extend_from_slice(&HELLO_VERSION.to_be_bytes());
    message.extend_from_slice(&chain_id.to_be_bytes());
    message.extend_from_slice(&local_key.verifying_key().to_bytes());
    message.extend_from_slice(&challenge_frame);
    let mut to_sign = Vec::with_capacity(DOMAIN.len() + message.len());
    to_sign.extend_from_slice(DOMAIN);
    to_sign.extend_from_slice(&message);
    message.extend_from_slice(&local_key.sign(&to_sign).to_bytes());
    send_frame(stream, &message)
}

#[cfg(test)]
mod tests {
    use super::super::tcp::{configure_stream, IO_TIMEOUT};
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn run_exchange(server_chain: u32, client_chain: u32, expected_key_seed: u8) -> bool {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            configure_stream(&connection).unwrap();
            let expected = SigningKey::from_bytes(&[expected_key_seed; 32])
                .verifying_key()
                .to_bytes();
            accept_authenticated_hello(&mut connection, server_chain, [9; 32], &expected).is_ok()
        });
        let mut connection = TcpStream::connect_timeout(&addr, IO_TIMEOUT).unwrap();
        configure_stream(&connection).unwrap();
        let key = SigningKey::from_bytes(&[7; 32]);
        respond_to_challenge(&mut connection, client_chain, &key).unwrap();
        server.join().unwrap()
    }

    #[test]
    fn two_tcp_nodes_authenticate_expected_identity() {
        assert!(run_exchange(1, 1, 7));
    }

    #[test]
    fn rejects_wrong_chain_and_unauthorized_peer() {
        assert!(!run_exchange(1, 2, 7));
        assert!(!run_exchange(1, 1, 8));
    }

    #[test]
    fn rejects_zero_server_challenge_before_writing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut connection = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let expected = SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes();
        assert!(accept_authenticated_hello(&mut connection, 1, [0; 32], &expected).is_err());
    }
}
