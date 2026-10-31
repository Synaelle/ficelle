// SPDX-License-Identifier: AGPL-3.0-or-later

//! The first start, made so that nobody but whoever runs the server can
//! become its admin. Until setup is locked:
//!
//! - the log shows a setup link, `<public_url>/setup#token=...`. Its 256
//!   random bits sit after `#`, which browsers never send, so no proxy or
//!   access log sees them. Only their hash is kept, compared in constant
//!   time. It works once, for an hour, and a new one is printed at each
//!   start and whenever the last one ran out unused.
//! - the link is traded for a setup session of an hour, where the admin sets
//!   up a passkey or an authenticator app before the account exists.
//! - every api route but the setup ones answers `setupPending`.
//!
//! The link and the session live in memory: a restart prints a new link and
//! forgets any session. The lock is a row in the database, set with the
//! first admin, and never lifted.

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use axum::http::HeaderMap;
use rusqlite::Connection;
use webauthn_rs::prelude::PasskeyRegistration;

use crate::accounts::token::{self, Kind, TokenHash};
use crate::clock::MINUTE;
use crate::config::Config;
use crate::db::DbError;

/// Life of a setup link and of a setup session.
pub const LIFE: i64 = 60 * MINUTE;

/// Wrong second factor answers before a session ends and a new link is needed.
const MAX_FAILURES: u32 = 5;

/// Setup in memory, shared by every request.
#[derive(Default)]
pub struct Setup {
    inner: Mutex<Inner>,
    done: AtomicBool,
}

#[derive(Default)]
struct Inner {
    link: Option<Secret>,
    session: Option<Session>,
}

struct Secret {
    hash: TokenHash,
    expires_at: i64,
}

/// What a setup session keeps between its requests.
pub struct Session {
    secret: Secret,
    failures: u32,
    /// The id the admin's account will have, the passkey is registered for it.
    pub user_id: String,
    /// Authenticator app secret waiting for its first code.
    pub totp: Option<String>,
    /// Passkey registration waiting for the browser's answer.
    pub passkey: Option<PasskeyRegistration>,
}

/// Equal hashes, in a time that does not depend on where they differ.
fn same(a: &TokenHash, b: &TokenHash) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Setup {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether setup is known to be locked. False may be stale: the command
    /// line can make the first admin while the server runs.
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// Setup is locked: the link and any session go.
    pub fn set_done(&self) {
        self.done.store(true, Ordering::Release);
        *self.inner() = Inner::default();
    }

    /// Whether a new link should be printed: none is usable and no session
    /// is under way.
    pub fn link_needed(&self, now: i64) -> bool {
        let inner = self.inner();
        let live = |s: Option<&Secret>| s.is_some_and(|s| s.expires_at > now);
        !(self.is_done()
            || live(inner.link.as_ref())
            || live(inner.session.as_ref().map(|s| &s.secret)))
    }

    /// A new link token, replacing the last one.
    pub fn new_link(&self, now: i64) -> String {
        let (token, hash) = token::new_token(Kind::SetupLink);
        self.inner().link = Some(Secret {
            hash,
            expires_at: now + LIFE,
        });
        token
    }

    /// Trades a link token for a session token, once. `None` for a wrong,
    /// used or expired token.
    pub fn start(&self, link: &str, now: i64) -> Option<(String, i64)> {
        let given = token::hash(link.trim());
        let mut inner = self.inner();
        let ok = inner
            .link
            .as_ref()
            .is_some_and(|l| same(&l.hash, &given) && l.expires_at > now);
        if !ok {
            return None;
        }
        inner.link = None;
        let (session, hash) = token::new_token(Kind::Setup);
        let expires_at = now + LIFE;
        inner.session = Some(Session {
            secret: Secret { hash, expires_at },
            failures: 0,
            user_id: crate::accounts::store::new_id(),
            totp: None,
            passkey: None,
        });
        Some((session, expires_at))
    }

    /// Runs `f` on the session behind `token`. `None` when there is none,
    /// it ended, or the token is not its own.
    pub fn with_session<R>(
        &self,
        token: &str,
        now: i64,
        f: impl FnOnce(&mut Session) -> R,
    ) -> Option<R> {
        let given = token::hash(token.trim());
        let mut inner = self.inner();
        let session = inner
            .session
            .as_mut()
            .filter(|s| same(&s.secret.hash, &given) && s.secret.expires_at > now)?;
        Some(f(session))
    }

    /// Counts a wrong second factor answer, ending the session after a few.
    pub fn failed(&self, token: &str) {
        let given = token::hash(token.trim());
        let mut inner = self.inner();
        let ended = inner.session.as_mut().is_some_and(|s| {
            if !same(&s.secret.hash, &given) {
                return false;
            }
            s.failures += 1;
            s.failures >= MAX_FAILURES
        });
        if ended {
            inner.session = None;
        }
    }
}

/// Whether setup is locked in the database.
///
/// # Errors
///
/// Database errors only.
pub fn is_locked(conn: &Connection) -> Result<bool, DbError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM instance WHERE key = 'setup_done')",
        [],
        |r| r.get(0),
    )?)
}

/// Locks setup for good.
///
/// # Errors
///
/// Database errors only.
pub fn lock(conn: &Connection) -> Result<(), DbError> {
    conn.execute(
        "INSERT OR IGNORE INTO instance (key, value)
         VALUES ('setup_done', strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
        [],
    )?;
    Ok(())
}

/// Whether a request may use setup: it reached the proxy over https, or it
/// comes from this machine, or the config allows plain http.
///
/// `X-Forwarded-Proto` is believed from a trusted proxy only. A request
/// without any forwarded header from a loopback address is someone on the
/// server itself. One that went through the proxy over plain http says so
/// in the header and is refused, even from the proxy's own machine.
pub fn secure(headers: &HeaderMap, peer: IpAddr, client: IpAddr, config: &Config) -> bool {
    if config.setup.allow_http {
        return true;
    }
    let proto = config
        .server
        .trusts(peer)
        .then(|| headers.get("x-forwarded-proto"))
        .flatten()
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_ascii_lowercase());
    match proto.as_deref() {
        Some("https") => true,
        Some(_) => false,
        None => client.is_loopback(),
    }
}

/// The link as printed: the token after `#`, under the public address when
/// it is known.
pub fn link_text(config: &Config, token: &str) -> String {
    let base = config.server.public_url.trim_end_matches('/');
    if base.is_empty() {
        format!(
            "http://{}{}/setup#token={token}",
            config.server.bind,
            config.base_path()
        )
    } else {
        format!("{base}/setup#token={token}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_works_once_for_an_hour() {
        let setup = Setup::default();
        let now = 1_000_000;
        assert!(setup.link_needed(now));
        let link = setup.new_link(now);
        assert!(link.starts_with("sl_") && link.len() == 46, "{link}");
        assert!(!setup.link_needed(now));
        assert!(setup.start("sl_wrong", now).is_none());
        assert!(setup.start(&link, now + LIFE).is_none(), "expired");
        let (session, _) = setup.start(&link, now).unwrap();
        assert!(setup.start(&link, now).is_none(), "used");
        assert!(!setup.link_needed(now), "a session is under way");
        assert!(setup.with_session(&session, now, |_| ()).is_some());
        assert!(setup.with_session(&link, now, |_| ()).is_none());
        assert!(setup.with_session(&session, now + LIFE, |_| ()).is_none());
        assert!(setup.link_needed(now + LIFE), "the session ran out");
    }

    #[test]
    fn wrong_answers_end_the_session() {
        let setup = Setup::default();
        let link = setup.new_link(0);
        let (session, _) = setup.start(&link, 0).unwrap();
        setup.failed("su_someone_else");
        for _ in 0..MAX_FAILURES - 1 {
            setup.failed(&session);
        }
        assert!(setup.with_session(&session, 0, |_| ()).is_some());
        setup.failed(&session);
        assert!(setup.with_session(&session, 0, |_| ()).is_none());
        setup.set_done();
        assert!(!setup.link_needed(0));
    }

    #[test]
    fn https_comes_from_a_trusted_proxy_or_this_machine() {
        let mut config = Config::default();
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let headers = |proto: Option<&str>| {
            let mut h = HeaderMap::new();
            if let Some(p) = proto {
                h.insert("x-forwarded-proto", p.parse().unwrap());
            }
            h
        };
        let local = ip("127.0.0.1");
        let far = ip("203.0.113.9");
        assert!(secure(&headers(Some("https")), local, far, &config));
        assert!(secure(&headers(Some("HTTPS, http")), local, far, &config));
        assert!(!secure(&headers(Some("http")), local, far, &config));
        assert!(
            !secure(&headers(Some("http")), local, local, &config),
            "through the proxy over http, even from the server"
        );
        assert!(!secure(&headers(None), far, far, &config));
        assert!(
            !secure(&headers(Some("https")), far, far, &config),
            "an untrusted peer cannot claim https"
        );
        assert!(
            secure(&headers(None), local, local, &config),
            "on the server"
        );
        config.setup.allow_http = true;
        assert!(secure(&headers(None), far, far, &config));
    }

    #[test]
    fn the_link_names_the_public_address() {
        let mut config = Config::default();
        assert_eq!(
            link_text(&config, "sl_x"),
            "http://127.0.0.1:8080/setup#token=sl_x"
        );
        config.server.public_url = "https://notes.example.org/".into();
        assert_eq!(
            link_text(&config, "sl_x"),
            "https://notes.example.org/setup#token=sl_x"
        );
    }
}
