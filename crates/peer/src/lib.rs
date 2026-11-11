// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sync between one person's own devices on the same network, with no
//! account and no server.
//!
//! Devices find each other with mDNS, advertising only a random name made
//! at each start, so the network learns nothing about whose device it is.
//! Discovery only gives an address to try: trust comes from the Noise
//! handshake, where each side checks the other's long-term key against the
//! devices it paired. Pairing runs SPAKE2 with a six digit code shown on one
//! device and typed on the other, see [`channel`].
//!
//! Inside the channel the devices say their versions of every document and
//! answer with the changes the other lacks, then the files, by hash. While
//! both stay open, each local write starts another such round.
//!
//! Everything runs on std threads: a listener, the mDNS daemon, a dialer,
//! and one thread per connection. The store is only held for the short
//! steps, never while waiting on the network.

pub mod channel;
mod node;
mod session;
mod wire;

pub use node::{Live, Node, PairingStatus, Status};

use app_core::blobs::BlobStore;
use app_core::store::Store;

/// Why something did not work.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The network.
    #[error("network: {0}")]
    Io(#[from] std::io::Error),
    /// The other side closed.
    #[error("the other device closed the connection")]
    Closed,
    /// Nothing came in time.
    #[error("the other device did not answer in time")]
    Timeout,
    /// The handshake failed: not a paired device, or a wrong code.
    #[error("handshake: {0}")]
    Handshake(String),
    /// The other side sent something this version does not take.
    #[error("the other device sent {0}")]
    Protocol(&'static str),
    /// The code was not the one shown.
    #[error("wrong code")]
    WrongCode,
    /// No device showing a code was found.
    #[error("no device is waiting for a code")]
    NotFound,
    /// The vault is locked.
    #[error("the notes are locked")]
    Locked,
    /// The vault refused.
    #[error(transparent)]
    Store(#[from] app_core::Error),
}

/// The app around the node: the vault, and who to tell when things happen.
pub trait Host: Send + Sync + 'static {
    /// Runs `f` with the store and the files, `None` while the vault is
    /// locked.
    fn with<T>(
        &self,
        f: impl FnOnce(&mut Store, &BlobStore) -> app_core::Result<T>,
    ) -> Option<app_core::Result<T>>;
    /// Notes changed here from another device.
    fn changed(&self);
    /// Files arrived from another device.
    fn files_arrived(&self);
    /// Something the status shows changed.
    fn status_changed(&self);
    /// Files up to this many bytes come with each sync, larger ones not.
    fn fetch_up_to(&self) -> u64;
    /// This device's name when the person gave none.
    fn default_name(&self) -> String;
}

/// This device as the others see it.
#[derive(Debug, Clone)]
pub struct Me {
    /// `windows`, `linux`, `macos` or `android`.
    pub platform: String,
    /// The app version.
    pub version: String,
}

/// Locked vault, as an error.
fn store<H: Host, T>(
    host: &H,
    f: impl FnOnce(&mut Store, &BlobStore) -> app_core::Result<T>,
) -> Result<T, Error> {
    Ok(host.with(f).ok_or(Error::Locked)??)
}

#[cfg(test)]
mod tests;
