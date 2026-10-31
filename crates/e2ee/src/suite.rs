// SPDX-License-Identifier: AGPL-3.0-or-later

//! The OPAQUE configuration. Changing any of it locks every account out, a
//! new configuration would be a new protocol version.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use opaque_ke::{CipherSuite, Identifiers, Ristretto255, TripleDh};

use crate::{Error, Result};

/// ristretto255 for the OPRF and the key exchange, SHA-512, and Argon2id
/// to stretch the password, so a stolen server record costs an Argon2id
/// run per guess. How hard that run is depends on the record, see [`Ksf`].
pub struct Suite;

/// How a record's password is stretched. Only the client runs it: the
/// server keeps which one each record was made with and says it when a
/// sign-in starts. A new record always takes [`Ksf::CURRENT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ksf {
    /// Argon2id at the crate's defaults, 19 MiB and 2 passes: records made
    /// before 0.3.0.
    V1 = 1,
    /// Argon2id at 64 MiB and 3 passes, about four times the cost of a
    /// guess.
    V2 = 2,
}

impl Ksf {
    /// What new records are made with.
    pub const CURRENT: Self = Self::V2;

    /// From its number on the wire. `None` for one this build does not know.
    pub fn from_number(number: u8) -> Option<Self> {
        match number {
            1 => Some(Self::V1),
            2 => Some(Self::V2),
            _ => None,
        }
    }

    /// Its number on the wire and in the database.
    pub fn number(self) -> u8 {
        self as u8
    }

    pub(crate) fn argon2(self) -> argon2::Argon2<'static> {
        match self {
            Self::V1 => argon2::Argon2::default(),
            Self::V2 => argon2::Argon2::new(
                argon2::Algorithm::Argon2id,
                argon2::Version::V0x13,
                argon2::Params::new(64 * 1024, 3, 1, None).expect("valid argon2 parameters"),
            ),
        }
    }
}

impl CipherSuite for Suite {
    type OprfCs = Ristretto255;
    type KeyExchange = TripleDh<Ristretto255, sha2_10::Sha512>;
    type Ksf = argon2::Argon2<'static>;
}

/// Bound into every key exchange, so a transcript from another protocol
/// using OPAQUE cannot be replayed here.
pub const CONTEXT: &[u8] = b"notes e2ee sign-in v1";

/// The account's username is the client identity: a record made for one
/// username does not sign in another. Pass it through [`username`] first.
pub fn identifiers(username: &str) -> Identifiers<'_> {
    Identifiers {
        client: Some(username.as_bytes()),
        server: None,
    }
}

/// The username as both ends bind it: trimmed, lower case, the way the
/// server stores it. Typed "Anna " or "anna", it is the same account.
pub fn username(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

/// Bytes as message text.
pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Message text as bytes. `what` names it in the error.
///
/// # Errors
///
/// [`Error::Malformed`] when it is not base64url.
pub fn decode(text: &str, what: &'static str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(text.trim())
        .map_err(|_| Error::Malformed(what))
}
