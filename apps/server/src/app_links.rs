// SPDX-License-Identifier: AGPL-3.0-or-later

//! Signing an app in with a code (RFC 8628, with PKCE S256 from RFC 7636).
//! The app asks for a code and shows it; the person types it on the
//! server's page, on the same device or another, signs in there and agrees.
//! Meanwhile the app asks every few seconds whether the code was approved,
//! with the verifier only it knows, and gets its own session once it is. No
//! browser has to hand anything back to the app, which some browsers on a
//! phone will not do. A code lives ten minutes and only its hash is kept.
//!
//! With a QR code, the other way round: the page asks for an offer and shows
//! it as a QR code, a phone scans it and asks for its code naming the offer,
//! the page sees which app answered, and the person signs in and approves
//! it, as with a typed code. An offer takes one answer and lives five
//! minutes, only its hash is kept. Someone else scanning it shows up on the
//! page under their own device's name, and gets nothing unless the person
//! signs in and approves them.

use app_protocol::account::{AppLinkInfo, AppLinkRequest, DeviceInfo};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::accounts::token::{self, Kind};
use crate::app_codes::{Redeemed, challenge_of, same};
use crate::db::DbError;

/// How long a code works, in milliseconds.
pub const LIFE: i64 = 10 * 60 * 1000;

/// How long a QR code works, in milliseconds.
pub const OFFER_LIFE: i64 = 5 * 60 * 1000;

/// QR codes shown at once: anyone may ask for one, the table stays small.
const MOST_OFFERS: i64 = 200;

/// Seconds between an app's questions.
pub const INTERVAL: u32 = 3;

/// Codes waiting at once: anyone may ask for one, the table stays small.
const MOST_WAITING: i64 = 200;

/// Codes, and QR codes, waiting at once from one network: a few devices
/// being set up together, never enough to take every place.
const MOST_FROM_ONE: i64 = 5;

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

/// What asking for a code gave.
#[derive(Debug, PartialEq, Eq)]
pub enum Created {
    /// The code.
    Code(String),
    /// Too many codes wait already.
    Busy,
    /// The QR code named is unknown, expired or answered already.
    OfferGone,
}

/// A new code for an app, the request already checked, joined to the QR code
/// it names. Run it in a transaction: a code whose offer is gone must not
/// stay.
///
/// # Errors
///
/// Database errors only.
pub fn create(
    conn: &Connection,
    req: &AppLinkRequest,
    network: &str,
    now: i64,
) -> Result<Created, DbError> {
    let (waiting, theirs): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE network = ?2) FROM app_links WHERE expires_at > ?1",
        params![now, network],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if waiting >= MOST_WAITING || theirs >= MOST_FROM_ONE {
        return Ok(Created::Busy);
    }
    let code = loop {
        let code = new_code();
        let added = conn.execute(
            "INSERT OR IGNORE INTO app_links
                 (hash, challenge, app_key, device_name, device_platform, expires_at, network)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                hash(&code).as_slice(),
                req.challenge,
                req.key,
                req.device.name,
                req.device.platform,
                now + LIFE,
                network
            ],
        )?;
        if added == 1 {
            break code;
        }
    };
    if let Some(offer) = &req.offer {
        let joined = conn.execute(
            "UPDATE link_offers SET link_hash = ?2
             WHERE hash = ?1 AND link_hash IS NULL AND expires_at > ?3",
            params![token::hash(offer).as_slice(), hash(&code).as_slice(), now],
        )?;
        if joined == 0 {
            return Ok(Created::OfferGone);
        }
    }
    Ok(Created::Code(code))
}

/// A new QR code's token, `None` when too many are shown already, or from
/// `network`.
///
/// # Errors
///
/// Database errors only.
pub fn offer(conn: &Connection, network: &str, now: i64) -> Result<Option<String>, DbError> {
    let (shown, theirs): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE network = ?2) FROM link_offers WHERE expires_at > ?1",
        params![now, network],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if shown >= MOST_OFFERS || theirs >= MOST_FROM_ONE {
        return Ok(None);
    }
    let (offer, hashed) = token::new_token(Kind::LinkOffer);
    conn.execute(
        "INSERT INTO link_offers (hash, expires_at, network) VALUES (?1, ?2, ?3)",
        params![hashed.as_slice(), now + OFFER_LIFE, network],
    )?;
    Ok(Some(offer))
}

/// Where a QR code stands, for the page that shows it.
#[derive(Debug)]
pub enum Offered {
    /// Unknown, expired or approved already.
    Gone,
    /// No app scanned it yet.
    Waiting,
    /// This app did, waiting for the person.
    Answered(AppLinkInfo),
}

/// The app that answered `offer`. The token is the page's secret: whoever
/// holds it learns only the device name the app gave.
///
/// # Errors
///
/// Database errors only.
pub fn offered(conn: &Connection, offer: &str, now: i64) -> Result<Offered, DbError> {
    let row: Option<Option<Vec<u8>>> = conn
        .query_row(
            "SELECT link_hash FROM link_offers WHERE hash = ?1 AND expires_at > ?2",
            params![token::hash(offer).as_slice(), now],
            |r| r.get(0),
        )
        .optional()?;
    let Some(link) = row else {
        return Ok(Offered::Gone);
    };
    let Some(link) = link else {
        return Ok(Offered::Waiting);
    };
    let app = conn
        .query_row(
            "SELECT device_name, device_platform, app_key, challenge FROM app_links
             WHERE hash = ?1 AND user_id IS NULL AND expires_at > ?2",
            params![link, now],
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
        .optional()?;
    Ok(app.map_or(Offered::Gone, Offered::Answered))
}

/// Approves the app that answered `offer` for the signed-in `user_id`, the
/// offer is used up. Returns the app's device name, `None` when there is nothing to
/// approve.
///
/// # Errors
///
/// Database errors only.
pub fn approve_offer(
    conn: &Connection,
    offer: &str,
    user_id: &str,
    sealed: Option<&str>,
    now: i64,
) -> Result<Option<String>, DbError> {
    let link: Option<Vec<u8>> = conn
        .query_row(
            "DELETE FROM link_offers
             WHERE hash = ?1 AND expires_at > ?2 AND link_hash IS NOT NULL
             RETURNING link_hash",
            params![token::hash(offer).as_slice(), now],
            |r| r.get(0),
        )
        .optional()?;
    let Some(link) = link else {
        return Ok(None);
    };
    Ok(conn
        .query_row(
            "UPDATE app_links SET user_id = ?2, sealed = ?3
             WHERE hash = ?1 AND user_id IS NULL AND expires_at > ?4
             RETURNING device_name",
            params![link, user_id, sealed, now],
            |r| r.get(0),
        )
        .optional()?)
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
    Ok(
        conn.execute("DELETE FROM app_links WHERE expires_at <= ?1", [now])?
            + conn.execute("DELETE FROM link_offers WHERE expires_at <= ?1", [now])?,
    )
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
