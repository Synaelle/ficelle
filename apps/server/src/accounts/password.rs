// SPDX-License-Identifier: AGPL-3.0-or-later

//! Passwords: Argon2id in PHC strings, which carry their own cost so a cost
//! change in the config applies to each account at its next sign-in.

use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::config::AuthConfig;

/// Shortest password accepted, the same rule as the app's own password.
pub const MIN_LEN: usize = 8;
/// Longest password accepted. Argon2 copes with any length, the bound keeps
/// a request from feeding it megabytes.
pub const MAX_BYTES: usize = 1024;

/// Hashes and checks passwords with the configured cost.
#[derive(Clone)]
pub struct Passwords {
    params: Params,
    /// Checked when the username is unknown, so that answer takes as long as
    /// a wrong password and does not reveal which accounts exist.
    dummy: String,
}

/// Outcome of a check.
#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    /// Right password.
    Good,
    /// Right password, stored with another cost: store this new hash.
    GoodRehash(String),
    /// Wrong password.
    Bad,
}

impl Passwords {
    /// # Panics
    ///
    /// Never for a validated config, which checked these parameters.
    pub fn new(config: &AuthConfig) -> Self {
        let params = Params::new(
            config.argon2_memory_kib,
            config.argon2_iterations,
            config.argon2_parallelism,
            None,
        )
        .expect("argon2 parameters are validated with the config");
        let mut passwords = Self {
            params,
            dummy: String::new(),
        };
        passwords.dummy = passwords.hash("not a real password").unwrap_or_default();
        passwords
    }

    fn argon(&self) -> Argon2<'static> {
        Argon2::new(Algorithm::Argon2id, Version::V0x13, self.params.clone())
    }

    /// A PHC string for storage.
    ///
    /// # Errors
    ///
    /// Only if the system random source fails.
    pub fn hash(&self, password: &str) -> Result<String, String> {
        self.argon()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .map_err(|e| e.to_string())
    }

    /// Checks `password` against `stored`, or against a dummy hash when there
    /// is no account, which always fails but costs the same time.
    pub fn check(&self, password: &str, stored: Option<&str>) -> Check {
        let hash = stored.unwrap_or(&self.dummy);
        let Ok(parsed) = PasswordHash::new(hash) else {
            return Check::Bad;
        };
        if self
            .argon()
            .verify_password(password.as_bytes(), &parsed)
            .is_err()
            || stored.is_none()
        {
            return Check::Bad;
        }
        // only the costs matter, a parsed hash also carries its output length
        let same = Params::try_from(&parsed).is_ok_and(|p| {
            (p.m_cost(), p.t_cost(), p.p_cost())
                == (
                    self.params.m_cost(),
                    self.params.t_cost(),
                    self.params.p_cost(),
                )
        });
        if same {
            return Check::Good;
        }
        self.hash(password).map_or(Check::Good, Check::GoodRehash)
    }
}

/// Why a new password is refused, `None` when it is fine.
pub fn weakness(password: &str, username: &str) -> Option<&'static str> {
    if password.chars().count() < MIN_LEN {
        Some("use at least 8 characters")
    } else if password.len() > MAX_BYTES {
        Some("use at most 1024 bytes")
    } else if password.eq_ignore_ascii_case(username) {
        Some("the password cannot be the username")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap costs, the real ones take a noticeable time per hash.
    pub fn fast() -> Passwords {
        Passwords::new(&AuthConfig {
            argon2_memory_kib: 64,
            argon2_iterations: 1,
            ..AuthConfig::default()
        })
    }

    #[test]
    fn right_and_wrong_passwords() {
        let passwords = fast();
        let stored = passwords.hash("correct horse").unwrap();
        assert!(stored.starts_with("$argon2id$"));
        assert_eq!(passwords.check("correct horse", Some(&stored)), Check::Good);
        assert_eq!(passwords.check("wrong horse", Some(&stored)), Check::Bad);
        assert_eq!(
            passwords.check("correct horse", None),
            Check::Bad,
            "no account never passes"
        );
    }

    #[test]
    fn a_cost_change_rehashes_on_the_next_good_check() {
        let old = fast();
        let stored = old.hash("correct horse").unwrap();
        let stronger = Passwords::new(&AuthConfig {
            argon2_memory_kib: 128,
            argon2_iterations: 1,
            ..AuthConfig::default()
        });
        let Check::GoodRehash(new) = stronger.check("correct horse", Some(&stored)) else {
            panic!("expected a rehash");
        };
        assert_eq!(stronger.check("correct horse", Some(&new)), Check::Good);
    }

    #[test]
    fn weak_passwords() {
        assert!(weakness("short", "anna").is_some());
        assert!(weakness("Annabelle", "annabelle").is_some());
        assert!(weakness("a fine password", "anna").is_none());
        assert!(
            weakness("ééééééé", "anna").is_some(),
            "counts characters, not bytes"
        );
    }
}
