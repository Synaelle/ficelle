// SPDX-License-Identifier: AGPL-3.0-or-later

//! Random secrets handed to clients: access and refresh tokens, invites,
//! setup links and sessions. Only their sha-256 is stored, a token has 256 random bits
//! so a fast hash is enough, nothing can be guessed from it.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// Hash stored in place of a token.
pub type TokenHash = [u8; 32];

/// What a token is for, also its visible prefix so a leaked one is easy to
/// recognise in logs or by secret scanners.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// Short lived api token.
    Access,
    /// Single use token for the next pair.
    Refresh,
    /// Invite link.
    Invite,
    /// A step of a sign-in or passkey ceremony.
    Challenge,
    /// Reads the feedback inbox, for `reports pull`.
    Feedback,
    /// The setup link printed in the log.
    SetupLink,
    /// A setup session, between the link and the first admin.
    Setup,
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Access => "at_",
            Self::Refresh => "rt_",
            Self::Invite => "inv_",
            Self::Challenge => "ch_",
            Self::Feedback => "fb_",
            Self::SetupLink => "sl_",
            Self::Setup => "su_",
        }
    }
}

/// A new random token and its hash.
///
/// # Panics
///
/// When the operating system cannot provide random bytes, nothing safe can
/// run then.
pub fn new_token(kind: Kind) -> (String, TokenHash) {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the system random source works");
    let token = format!("{}{}", kind.prefix(), URL_SAFE_NO_PAD.encode(bytes));
    let hash = hash(&token);
    (token, hash)
}

/// The stored form of a token.
pub fn hash(token: &str) -> TokenHash {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_prefixed_and_hash_the_same() {
        let (a, ha) = new_token(Kind::Refresh);
        let (b, _) = new_token(Kind::Refresh);
        assert_ne!(a, b);
        assert!(a.starts_with("rt_"));
        assert_eq!(hash(&a), ha);
    }
}
