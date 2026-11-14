// SPDX-License-Identifier: AGPL-3.0-or-later

//! Signing an app in through the server's own page (OAuth 2.0 for native
//! apps, RFC 8252, with PKCE S256 from RFC 7636). The app opens `/connect`
//! in the browser; once the person signed in there and agreed, the page
//! asks for a code, which goes back to the app through its redirect. The app
//! trades it, with the verifier only it knows, for its own session. A code
//! works once, for two minutes, and only its hash is kept.

use app_protocol::account::{AppCodeRequest, DeviceInfo};
use data_encoding::BASE64URL_NOPAD;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::accounts::token::{self, Kind};
use crate::db::DbError;

/// How long a code works, in milliseconds.
const LIFE: i64 = 2 * 60 * 1000;

/// The longest sealed key passed along: an export key sealed is far less.
const MAX_SEALED: usize = 1024;

/// Where the browser may send a code: the app's loopback listener on a
/// computer, `http://127.0.0.1:<port>/callback`, or the app's own scheme on a
/// phone, `<identifier>:/connect` for one of this project's apps. Anything
/// else could hand a person's session to someone else.
pub fn redirect_ok(redirect: &str) -> bool {
    if let Some(rest) = redirect.strip_prefix("http://127.0.0.1:") {
        return rest
            .strip_suffix("/callback")
            .and_then(|port| port.parse::<u16>().ok())
            .is_some_and(|port| port >= 1024);
    }
    let Some(scheme) = redirect.strip_suffix(":/connect") else {
        return false;
    };
    // the release, preview and dev apps share one root identifier
    let root = app_brand::IDENTIFIER
        .trim_end_matches(".dev")
        .trim_end_matches(".preview");
    [
        root.to_owned(),
        format!("{root}.preview"),
        format!("{root}.dev"),
    ]
    .iter()
    .any(|allowed| allowed == scheme)
}

/// A PKCE challenge, S256 of a verifier: 43 base64url characters.
pub fn challenge_ok(challenge: &str) -> bool {
    challenge.len() == 43
        && BASE64URL_NOPAD
            .decode(challenge.as_bytes())
            .is_ok_and(|b| b.len() == 32)
}

/// Whether a sealed key could be one, before it is passed along untouched.
pub fn sealed_ok(sealed: &str) -> bool {
    !sealed.is_empty()
        && sealed.len() <= MAX_SEALED
        && sealed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'+' | b'/' | b'='))
}

/// The challenge a verifier makes (RFC 7636, S256). A verifier has 43 to
/// 128 unreserved characters.
fn challenge_of(verifier: &str) -> Option<String> {
    let fits = (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'));
    fits.then(|| BASE64URL_NOPAD.encode(&Sha256::digest(verifier.as_bytes())))
}

/// A new code for `user_id`, the request already checked. Returns it plain,
/// to hand out once.
///
/// # Errors
///
/// Database errors only.
pub fn issue(
    conn: &Connection,
    user_id: &str,
    req: &AppCodeRequest,
    now: i64,
) -> Result<String, DbError> {
    let (code, hash) = token::new_token(Kind::AppCode);
    conn.execute(
        "INSERT INTO app_codes
             (hash, user_id, device_name, device_platform, challenge, redirect, sealed, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            hash.as_slice(),
            user_id,
            req.device.name,
            req.device.platform,
            req.challenge,
            req.redirect,
            req.sealed,
            now + LIFE
        ],
    )?;
    Ok(code)
}

/// What a code that checked out stood for.
#[derive(Debug)]
pub struct Redeemed {
    pub user_id: String,
    pub device: DeviceInfo,
    pub sealed: Option<String>,
}

/// Takes a code: `None` unless it exists, has not expired, was issued for
/// this redirect and the verifier makes its challenge. A code is gone after
/// one try, right or wrong, so a guess never gets a second one.
///
/// # Errors
///
/// Database errors only.
pub fn redeem(
    conn: &Connection,
    code: &str,
    verifier: &str,
    redirect: &str,
    now: i64,
) -> Result<Option<Redeemed>, DbError> {
    let hash = token::hash(code);
    let found = conn
        .query_row(
            "DELETE FROM app_codes WHERE hash = ?1
             RETURNING user_id, device_name, device_platform, challenge, redirect, sealed, expires_at",
            [hash.as_slice()],
            |r| {
                Ok(Stored {
                    redeemed: Redeemed {
                        user_id: r.get(0)?,
                        device: DeviceInfo {
                            name: r.get(1)?,
                            platform: r.get(2)?,
                            replaces: None,
                        },
                        sealed: r.get(5)?,
                    },
                    challenge: r.get(3)?,
                    redirect: r.get(4)?,
                    expires_at: r.get(6)?,
                })
            },
        )
        .optional()?;
    let Some(stored) = found else {
        return Ok(None);
    };
    let matches = challenge_of(verifier).is_some_and(|made| same(&made, &stored.challenge));
    if stored.expires_at <= now || stored.redirect != redirect || !matches {
        return Ok(None);
    }
    Ok(Some(stored.redeemed))
}

/// A code's row as kept.
struct Stored {
    redeemed: Redeemed,
    challenge: String,
    redirect: String,
    expires_at: i64,
}

/// Compares in time that does not depend on where they differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Removes expired codes. Returns how many.
///
/// # Errors
///
/// Database errors only.
pub fn cleanup(conn: &Connection, now: i64) -> Result<usize, DbError> {
    Ok(conn.execute("DELETE FROM app_codes WHERE expires_at <= ?1", [now])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_apps_own_redirects() {
        for good in [
            "http://127.0.0.1:53111/callback",
            "http://127.0.0.1:1024/callback",
            "com.synaelle.ficelle:/connect",
            "com.synaelle.ficelle.dev:/connect",
            "com.synaelle.ficelle.preview:/connect",
        ] {
            assert!(redirect_ok(good), "{good}");
        }
        for bad in [
            "http://127.0.0.1:80/callback",
            "http://127.0.0.1:53111/elsewhere",
            "http://localhost:53111/callback",
            "http://127.0.0.1.evil.example:53111/callback",
            "https://evil.example/callback",
            "com.evil.app:/connect",
            "com.synaelle.ficelle.evil:/connect",
            "com.synaelle.ficelle:/connect/extra",
            "",
        ] {
            assert!(!redirect_ok(bad), "{bad}");
        }
    }

    #[test]
    fn the_challenge_is_the_verifiers_s256() {
        // RFC 7636 appendix B
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_of(verifier).as_deref(),
            Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
        );
        assert!(challenge_ok("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));
        assert!(!challenge_ok("short"));
        assert!(challenge_of("too short").is_none());
        assert!(challenge_of(&"a".repeat(129)).is_none());
    }
}
