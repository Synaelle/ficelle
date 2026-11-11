// SPDX-License-Identifier: AGPL-3.0-or-later

//! The encrypted channel between two devices: a TCP stream, a Noise
//! handshake, then messages of any size cut into Noise frames.
//!
//! Every frame on the wire is a 2 byte length and that many bytes.
//!
//! A sync connection is `Noise_IK`: the dialling device already holds the
//! key of the paired device it means to reach, and its first message, with
//! its own key inside, only that device can read. The answering side checks
//! that key against the devices it paired before it says anything. So a
//! stranger who connects, or who answers in place of a device, learns no
//! key: nothing on the network follows a device from one day to the next.
//!
//! A pairing connection first runs SPAKE2 with the code, and the key it
//! gives becomes the pre-shared key of `Noise_XXpsk0`, mixed in from the
//! first message: with a wrong code the very first message does not read,
//! and neither side has shown its key.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use snow::{Builder, HandshakeState, TransportState};
use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::Error;

const SYNC: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
const PAIRING: &str = "Noise_XXpsk0_25519_ChaChaPoly_BLAKE2s";
/// Largest Noise message, and the plaintext a frame carries.
const FRAME: usize = 65_535;
const TAG: usize = 16;
/// A message larger than this is refused, a device never sends one.
const MAX_MESSAGE: usize = 96 * 1024 * 1024;
/// How long one read waits before the caller gets to do something else.
const POLL: Duration = Duration::from_millis(200);
/// Handshakes and a frame started must finish within this.
const PATIENCE: Duration = Duration::from_secs(15);

/// First byte of a connection: what the dialling side wants.
pub(crate) const WANTS_SYNC: u8 = b'S';
/// See [`WANTS_SYNC`].
pub(crate) const WANTS_PAIRING: u8 = b'P';

/// What ties the handshake to this app and identity: a dev build and the
/// release never complete one with each other.
fn prologue() -> Vec<u8> {
    format!(
        "{} local sync {}",
        app_brand::SLUG,
        app_protocol::local::PROTOCOL
    )
    .into_bytes()
}

fn noise(err: &snow::Error) -> Error {
    Error::Handshake(err.to_string())
}

/// The public key of a secret one.
///
/// # Errors
///
/// A key of the wrong size.
pub fn public_key(secret: &[u8; 32]) -> Result<[u8; 32], Error> {
    use snow::resolvers::{CryptoResolver, DefaultResolver};
    let mut dh = DefaultResolver
        .resolve_dh(&snow::params::DHChoice::Curve25519)
        .ok_or_else(|| Error::Handshake("no curve25519".into()))?;
    dh.set(secret);
    dh.pubkey()
        .try_into()
        .map_err(|_| Error::Handshake("public key size".into()))
}

/// A new long-term secret key.
///
/// # Errors
///
/// No randomness from the system.
pub fn new_secret() -> Result<[u8; 32], Error> {
    let pair = Builder::new(SYNC.parse().map_err(|e| noise(&e))?)
        .generate_keypair()
        .map_err(|e| noise(&e))?;
    pair.private
        .try_into()
        .map_err(|_| Error::Handshake("secret key size".into()))
}

/// Frames on a TCP stream, with reads that give up after [`POLL`] so a
/// session can send between them.
struct Wire {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl Wire {
    fn new(stream: TcpStream) -> Result<Self, Error> {
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(POLL))?;
        stream.set_write_timeout(Some(PATIENCE))?;
        Ok(Self {
            stream,
            buf: Vec::new(),
        })
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), Error> {
        let len = u16::try_from(frame.len()).map_err(|_| Error::Protocol("frame too long"))?;
        let mut out = Vec::with_capacity(frame.len() + 2);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(frame);
        self.stream.write_all(&out)?;
        Ok(())
    }

    /// A whole frame when one arrived, `None` after a quiet [`POLL`].
    fn poll(&mut self) -> Result<Option<Vec<u8>>, Error> {
        let started = Instant::now();
        loop {
            if self.buf.len() >= 2 {
                let len = usize::from(u16::from_be_bytes([self.buf[0], self.buf[1]]));
                if self.buf.len() >= len + 2 {
                    let frame = self.buf[2..len + 2].to_vec();
                    self.buf.drain(..len + 2);
                    return Ok(Some(frame));
                }
            }
            let mut chunk = [0u8; 16 * 1024];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(Error::Closed),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    // a frame half here must finish, an empty buffer may wait
                    if self.buf.is_empty() {
                        return Ok(None);
                    }
                    if started.elapsed() > PATIENCE {
                        return Err(Error::Protocol("a frame stopped halfway"));
                    }
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// A frame, waiting up to [`PATIENCE`].
    fn recv(&mut self) -> Result<Vec<u8>, Error> {
        let started = Instant::now();
        loop {
            if let Some(frame) = self.poll()? {
                return Ok(frame);
            }
            if started.elapsed() > PATIENCE {
                return Err(Error::Timeout);
            }
        }
    }
}

/// An open channel: messages in, messages out, both sides authenticated.
pub struct Channel {
    wire: Wire,
    noise: TransportState,
    /// The other device's long-term key, proven by the handshake.
    pub remote_key: [u8; 32],
    /// Plaintext received, not yet a whole message.
    pending: Vec<u8>,
}

/// Runs a handshake. `welcome` sees the other side's key as soon as it is
/// known, before this side answers: false ends it there.
fn run_handshake(
    mut wire: Wire,
    mut hs: HandshakeState,
    welcome: impl Fn(&[u8; 32]) -> bool,
) -> Result<Channel, Error> {
    let mut buf = vec![0u8; FRAME];
    let mut checked = false;
    while !hs.is_handshake_finished() {
        if !checked && let Some(key) = hs.get_remote_static() {
            checked = true;
            let key: [u8; 32] = key
                .try_into()
                .map_err(|_| Error::Handshake("key size".into()))?;
            if !welcome(&key) {
                return Err(Error::Handshake("not a paired device".into()));
            }
        }
        if hs.is_my_turn() {
            let n = hs.write_message(&[], &mut buf).map_err(|e| noise(&e))?;
            wire.send(&buf[..n])?;
        } else {
            let frame = wire.recv()?;
            hs.read_message(&frame, &mut buf).map_err(|e| noise(&e))?;
        }
    }
    let remote_key: [u8; 32] = hs
        .get_remote_static()
        .ok_or_else(|| Error::Handshake("no key from the other device".into()))?
        .try_into()
        .map_err(|_| Error::Handshake("key size".into()))?;
    let noise = hs.into_transport_mode().map_err(|e| noise(&e))?;
    Ok(Channel {
        wire,
        noise,
        remote_key,
        pending: Vec::new(),
    })
}

fn builder<'a>(
    pattern: &str,
    secret: &'a [u8; 32],
    prologue: &'a [u8],
) -> Result<Builder<'a>, Error> {
    Builder::new(pattern.parse().map_err(|e| noise(&e))?)
        .local_private_key(secret)
        .map_err(|e| noise(&e))?
        .prologue(prologue)
        .map_err(|e| noise(&e))
}

/// Dials the paired device whose key is `theirs`.
///
/// # Errors
///
/// The connection failed, or the other side is not that device.
pub fn dial_sync(
    stream: TcpStream,
    secret: &[u8; 32],
    theirs: &[u8; 32],
) -> Result<Channel, Error> {
    let mut wire = Wire::new(stream)?;
    wire.stream.write_all(&[WANTS_SYNC])?;
    let prologue = prologue();
    let hs = builder(SYNC, secret, &prologue)?
        .remote_public_key(theirs)
        .map_err(|e| noise(&e))?
        .build_initiator()
        .map_err(|e| noise(&e))?;
    run_handshake(wire, hs, |_| true)
}

/// Answers a sync channel, after its first byte was read. `paired` decides
/// on the dialling device's key before anything is sent back.
///
/// # Errors
///
/// The handshake failed, or `paired` refused.
pub fn answer_sync(
    stream: TcpStream,
    secret: &[u8; 32],
    paired: impl Fn(&[u8; 32]) -> bool,
) -> Result<Channel, Error> {
    let wire = Wire::new(stream)?;
    let prologue = prologue();
    let hs = builder(SYNC, secret, &prologue)?
        .build_responder()
        .map_err(|e| noise(&e))?;
    run_handshake(wire, hs, paired)
}

/// The key both sides get from the code, the same only when both typed the
/// same one.
fn code_key(wire: &mut Wire, code: &str) -> Result<[u8; 32], Error> {
    let (spake, outbound) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.as_bytes()),
        &Identity::new(prologue().as_slice()),
    );
    wire.send(&outbound)?;
    let inbound = wire.recv()?;
    let key = spake
        .finish(&inbound)
        .map_err(|e| Error::Handshake(format!("pairing: {e:?}")))?;
    key.get(..32)
        .and_then(|k| k.try_into().ok())
        .ok_or_else(|| Error::Handshake("pairing key size".into()))
}

/// Dials a device showing a code.
///
/// # Errors
///
/// [`Error::Handshake`] for a wrong code, the connection failed.
pub fn dial_pairing(stream: TcpStream, secret: &[u8; 32], code: &str) -> Result<Channel, Error> {
    let mut wire = Wire::new(stream)?;
    wire.stream.write_all(&[WANTS_PAIRING])?;
    let psk = code_key(&mut wire, code)?;
    let prologue = prologue();
    let hs = builder(PAIRING, secret, &prologue)?
        .psk(0, &psk)
        .map_err(|e| noise(&e))?
        .build_initiator()
        .map_err(|e| noise(&e))?;
    run_handshake(wire, hs, |_| true)
}

/// Answers a device that typed a code, after its first byte was read.
///
/// # Errors
///
/// [`Error::Handshake`] for a wrong code, the connection failed.
pub fn answer_pairing(stream: TcpStream, secret: &[u8; 32], code: &str) -> Result<Channel, Error> {
    let mut wire = Wire::new(stream)?;
    let psk = code_key(&mut wire, code)?;
    let prologue = prologue();
    let hs = builder(PAIRING, secret, &prologue)?
        .psk(0, &psk)
        .map_err(|e| noise(&e))?
        .build_responder()
        .map_err(|e| noise(&e))?;
    run_handshake(wire, hs, |_| true)
}

impl Channel {
    /// Sends one message, cut into frames.
    ///
    /// # Errors
    ///
    /// The connection broke.
    pub fn send(&mut self, message: &[u8]) -> Result<(), Error> {
        let len = u32::try_from(message.len()).map_err(|_| Error::Protocol("message too long"))?;
        let mut plain = Vec::with_capacity(message.len() + 4);
        plain.extend_from_slice(&len.to_be_bytes());
        plain.extend_from_slice(message);
        let mut buf = vec![0u8; FRAME];
        for chunk in plain.chunks(FRAME - TAG) {
            let n = self
                .noise
                .write_message(chunk, &mut buf)
                .map_err(|e| noise(&e))?;
            self.wire.send(&buf[..n])?;
        }
        Ok(())
    }

    /// A whole message when one arrived, `None` after a quiet moment.
    ///
    /// # Errors
    ///
    /// The connection broke or a frame did not decrypt.
    pub fn poll(&mut self) -> Result<Option<Vec<u8>>, Error> {
        let started = Instant::now();
        loop {
            if self.pending.len() >= 4 {
                let len = u32::from_be_bytes([
                    self.pending[0],
                    self.pending[1],
                    self.pending[2],
                    self.pending[3],
                ]) as usize;
                if len > MAX_MESSAGE {
                    return Err(Error::Protocol("message too long"));
                }
                if self.pending.len() >= len + 4 {
                    let message = self.pending[4..len + 4].to_vec();
                    self.pending.drain(..len + 4);
                    return Ok(Some(message));
                }
            }
            let Some(frame) = self.wire.poll()? else {
                if self.pending.is_empty() {
                    return Ok(None);
                }
                if started.elapsed() > PATIENCE {
                    return Err(Error::Timeout);
                }
                continue;
            };
            let mut buf = vec![0u8; FRAME];
            let n = self
                .noise
                .read_message(&frame, &mut buf)
                .map_err(|e| noise(&e))?;
            self.pending.extend_from_slice(&buf[..n]);
        }
    }

    /// A message, waiting up to the handshake's patience.
    ///
    /// # Errors
    ///
    /// As [`Self::poll`], or nothing came.
    pub fn recv(&mut self) -> Result<Vec<u8>, Error> {
        let started = Instant::now();
        loop {
            if let Some(message) = self.poll()? {
                return Ok(message);
            }
            if started.elapsed() > PATIENCE {
                return Err(Error::Timeout);
            }
        }
    }

    /// Where the other side is, `host:port`.
    pub fn peer_address(&self) -> Option<std::net::SocketAddr> {
        self.wire.stream.peer_addr().ok()
    }

    /// Closes the connection.
    pub fn close(&self) {
        let _ = self.wire.stream.shutdown(std::net::Shutdown::Both);
    }
}
