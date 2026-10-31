// SPDX-License-Identifier: AGPL-3.0-or-later

//! Local core shared by the desktop, mobile and web clients.
//!
//! Holds the data models, the encrypted store with search and history, key
//! handling and markdown export. It has no ui code, so the same logic runs
//! natively under tauri and, later, as wasm in the browser.
//!
//! Typical use:
//!
//! ```no_run
//! use app_core::vault::{Encryption, Vault};
//! use app_core::crypto::KdfParams;
//! use app_core::store::NewNote;
//!
//! # fn main() -> app_core::Result<()> {
//! let dir = std::path::Path::new("data");
//! let mut store = if Vault::exists(dir) {
//!     Vault::open(dir)?.unlock_with_password("password")?
//! } else {
//!     Vault::create(dir, Encryption::On { password: "password", kdf: KdfParams::DEFAULT })?.1
//! };
//! store.create_note(&NewNote { title: "Hello".into(), ..NewNote::default() })?;
//! # Ok(())
//! # }
//! ```

pub mod backup;
pub mod blobs;
pub mod catalog;
pub mod clock;
pub mod crypto;
pub mod db;
pub mod error;
pub mod export;
pub mod id;
pub mod import;
pub mod keystore;
pub mod links;
pub mod model;
pub mod preview;
pub mod recovery;
pub mod repo;
pub mod store;
pub mod sync;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod themes;
pub mod vault;

pub use error::{Error, Result};
