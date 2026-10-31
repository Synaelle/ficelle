// SPDX-License-Identifier: AGPL-3.0-or-later

//! End-to-end encrypted accounts, the parts both ends must agree on.
//!
//! Sign-in is OPAQUE (RFC 9807, `opaque-ke`): the password never leaves the
//! client, the server keeps a record it cannot test guesses against without
//! running the protocol with it. Each successful run gives the client an
//! export key the server never sees, the root of the account's keys:
//!
//! ```text
//! export key --HKDF--> password KEK --wraps--> account master key (random)
//! recovery code --HKDF--> recovery KEK --wraps--> account master key
//! ```
//!
//! Everything here is plain Rust without I/O so it builds for the apps, the
//! server and WebAssembly alike. Messages travel as base64url text.

mod client;
mod files;
mod keys;
mod recovery;
pub mod server;
mod share;
mod suite;
mod sync;

pub use client::{
    LoginFinish, LoginStart, RegistrationFinish, RegistrationStart, login_finish, login_start,
    registration_finish, registration_start,
};
pub use files::{
    CHUNK, FORMAT as FILE_FORMAT, FileCipher, FilesKey, HEADER as FILE_HEADER, Opener,
    SEALED_CHUNK, Sealer, file_address, files_key, files_key_from, open_file, plain_size,
    seal_file, sealed_size, space_files_key,
};
pub use keys::{AccountKeys, KeyBundle, MasterKey, NewAccount, new_account};
pub use opaque_ke;
pub use recovery::{generate as recovery_code, normalize as normalize_recovery_code};
pub use share::{
    KeyPair, SpaceKey, accept, accept_unsigned, fingerprint, grant, new_key_pair, new_space_key,
    open_key_pair, public_key_ok, seal_key_pair, space_doc_key,
};
pub use suite::{CONTEXT, Ksf, Suite, decode, encode, identifiers, username};
pub use sync::{
    DocKey, doc_key, new_doc_key, open_change, seal_change, unwrap_doc_key, wrap_doc_key,
};

/// Why an end-to-end operation failed.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// A message or key that does not decode.
    #[error("malformed {0}")]
    Malformed(&'static str),
    /// The password is wrong, or the server's answer does not check out.
    /// OPAQUE cannot tell them apart, on purpose.
    #[error("wrong password, or the server answer does not check out")]
    Refused,
    /// A wrapped key that does not open with this key.
    #[error("this key does not open the account")]
    WrongKey,
    /// Text that cannot be a recovery code.
    #[error("this is not a recovery code")]
    RecoveryCode,
    /// The system random source failed.
    #[error("no randomness available")]
    Random,
}

/// Result with [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
