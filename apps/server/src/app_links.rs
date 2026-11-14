// SPDX-License-Identifier: AGPL-3.0-or-later

//! Signing an app in with a code (RFC 8628, with PKCE S256 from RFC 7636).
//! The app asks for a code and shows it; the person types it on the
//! server's page, on the same device or another, signs in there and agrees.
//! Meanwhile the app asks every few seconds whether the code was approved,
//! with the verifier only it knows, and gets its own session once it is. No
//! browser has to hand anything back to the app, which some browsers on a
//! phone will not do. A code lives ten minutes and only its hash is kept.

use app_protocol::account::{AppLinkInfo, AppLinkRequest, DeviceInfo};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::app_codes::{Redeemed, challenge_of, same};
use crate::db::DbError;

/// How long a code works, in milliseconds.
pub const LIFE: i64 = 10 * 60 * 1000;

/// Seconds between an app's questions.
pub const INTERVAL: u32 = 3;

/// Codes waiting at once: anyone may ask for one, the table stays small.
const MOST_WAITING: i64 = 200;

/// Consonants only, as RFC 8628 suggests: no words, no 0/O or 1/I mix-ups.
const LETTERS: &[u8] = b"BCDFGHJKLMNPQRSTVWXZ";
const LENGTH: usize = 8;

/// A code as typed: any case, spaces and the dash left out. `None` when it
/// cannot be one.
pub fn normalize(typed: &str) -> Option<String> {
    let code: String = typed
        .chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (code.len() == LENGTH && code.bytes().all(|b| LETTERS.contains(&b))).then_some(code)
}

fn hash(code: &str) -> [u8; 32] {
    Sha256::digest(code.as_bytes()).into()
}

fn new_code() -> String {
    let mut code = String::with_capacity(LENGTH);
    while code.len() < LENGTH {
        let mut byte = [0u8; 1];
        getrandom::fill(&mut byte).expect("the system random source works");
        // 240 is the largest multiple of 20 below 256: no letter more likely
        if byte[0] < 240 {
            code.push(char::from(LETTERS[usize::from(byte[0] % 20)]));
        }
    }
    code
}

/// A new code for an app, the request already checked. `None` when too many
/// wait already.
///
/// # Errors
///
/// Database errors only.
pub fn create(
    conn: &Connection,
    req: &AppLinkRequest,
    now: i64,
) -> Result<Option<String>, DbError> {
    let waiting: i64 = conn.query_row(
        "SELECT COUNT(*) FROM app_links WHERE expires_at > ?1",
        [now],
        |r| r.get(0),
    )?;
    if waiting >= MOST_WAITING {
        return Ok(None);
    }
    loop {
        let code = new_code();
        let added = conn.execute(
            "INSERT OR IGNORE INTO app_links
                 (hash, challenge, app_key, device_name, device_platform, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                hash(&code).as_slice(),
                req.challenge,
                req.key,
                req.device.name,
                req.device.platform,
                now + LIFE
            ],
        )?;
        if added == 1 {
            return Ok(Some(code));
        }
    }
}

/// The app waiting behind a typed code, not approved yet and not expired.
///
/// # Errors
///
/// Database errors only.
pub fn find(conn: &Connection, code: &str, now: i64) -> Result<Option<AppLinkInfo>, DbError> {
    Ok(conn
        .query_row(
            "SELECT device_name, device_platform, app_key, challenge FROM app_links
             WHERE hash = ?1 AND user_id IS NULL AND expires_at > ?2",
            params![hash(code).as_slice(), now],
            |r| {
                Ok(AppLinkInfo {
                    device: DeviceInfo {
                        name: r.get(0)?,
                        platform: r.get(1)?,
                        replaces: None,
                    },
                    key: r.get(2)?,
                    challenge: r.get(3)?,
                })
            },
        )
        .optional()?)
}

/// Approves a waiting code for `user_id`. Returns the app's device name,
/// `None` when the code is unknown, expired or approved already.
///
/// # Errors
///
/// Database errors only.
pub fn approve(
    conn: &Connection,
    code: &str,
    user_id: &str,
    sealed: Option<&str>,
    now: i64,
) -> Result<Option<String>, DbError> {
    Ok(conn
        .query_row(
            "UPDATE app_links SET user_id = ?2, sealed = ?3
             WHERE hash = ?1 AND user_id IS NULL AND expires_at > ?4
             RETURNING device_name",
            params![hash(code).as_slice(), user_id, sealed, now],
            |r| r.get(0),
        )
        .optional()?)
}

/// What an app hears when it asks about its code.
#[derive(Debug)]
pub enum Asked {
    /// No such code for this verifier, or it expired.
    Unknown,
    /// Not approved yet.
    Pending,
    /// Approved: the code is gone, the session can be made.
    Approved(Redeemed),
}

/// The answer to an app asking about `code` with its `verifier`. Without the
/// right verifier a code says nothing, not even that it exists.
///
/// # Errors
///
/// Database errors only.
pub fn ask(conn: &Connection, code: &str, verifier: &str, now: i64) -> Result<Asked, DbError> {
    let key = hash(code);
    let found = conn
        .query_row(
            "SELECT challenge, user_id, device_name, device_platform, sealed, expires_at
             FROM app_links WHERE hash = ?1",
            [key.as_slice()],
            |r| {
                Ok(Kept {
                    challenge: r.get(0)?,
                    user_id: r.get(1)?,
                    device: DeviceInfo {
                        name: r.get(2)?,
                        platform: r.get(3)?,
                        replaces: None,
                    },
                    sealed: r.get(4)?,
                    expires_at: r.get(5)?,
                })
            },
        )
        .optional()?;
    let Some(Kept {
        challenge,
        user_id,
        device,
        sealed,
        expires_at,
    }) = found
    else {
        return Ok(Asked::Unknown);
    };
    let proven = challenge_of(verifier).is_some_and(|made| same(&made, &challenge));
    if !proven || expires_at <= now {
        return Ok(Asked::Unknown);
    }
    let Some(user_id) = user_id else {
        return Ok(Asked::Pending);
    };
    conn.execute("DELETE FROM app_links WHERE hash = ?1", [key.as_slice()])?;
    Ok(Asked::Approved(Redeemed {
        user_id,
        device,
        sealed,
    }))
}

/// A code's row as kept.
struct Kept {
    challenge: String,
    user_id: Option<String>,
    device: DeviceInfo,
    sealed: Option<String>,
    expires_at: i64,
}

/// Removes expired codes. Returns how many.
///
/// # Errors
///
/// Database errors only.
pub fn cleanup(conn: &Connection, now: i64) -> Result<usize, DbError> {
    Ok(conn.execute("DELETE FROM app_links WHERE expires_at <= ?1", [now])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_eight_easy_letters() {
        for _ in 0..200 {
            let code = new_code();
            assert_eq!(normalize(&code).as_deref(), Some(code.as_str()));
        }
        assert_eq!(normalize("kdrw-7qpm"), None, "no digits");
        assert_eq!(normalize("kdrw-bqpm").as_deref(), Some("KDRWBQPM"));
        assert_eq!(normalize("KDRW BQPM").as_deref(), Some("KDRWBQPM"));
        assert_eq!(normalize("KDRWBQP"), None);
        assert_eq!(normalize("AEIOUBCD"), None, "no vowels");
    }
}
