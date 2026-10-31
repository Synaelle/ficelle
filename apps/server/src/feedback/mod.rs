// SPDX-License-Identifier: AGPL-3.0-or-later

//! Feedback and bug reports from anyone's apps, no account needed. Reports
//! live in their own database ([`crate::db::FEEDBACK_FILE`]), never joined
//! with accounts. Against floods: a proof of work per report, limits per
//! address and per install and for the whole instance, a spam score that
//! files reports away rather than refusing them, and block lists that
//! answer as if the report was taken. Addresses are kept only as a keyed
//! hash, enough to count and block.

pub mod images;
pub mod inbox;

use std::net::IpAddr;

use app_protocol::feedback::{
    Category, Challenge, FeedbackInfo, MAX_ABOUT, MAX_CONTACT, MAX_IMAGE_BYTES, MAX_IMAGES,
    MAX_LOGS, MAX_MESSAGE, Report, SendTo,
};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::clock::{DAY, MINUTE};
use crate::config::FeedbackConfig;
use crate::db::DbError;

const HOUR: i64 = 60 * MINUTE;

/// Report bodies read at once, before their proof of work is checked.
pub const READING_AT_ONCE: usize = 4;

/// How long a challenge is accepted.
pub const CHALLENGE_LIFE: i64 = 10 * MINUTE;

/// The proof of work never asks more than this for busy senders, unless the
/// instance's own starting point is higher.
const MAX_BUSY_BITS: u8 = 24;

/// A spam score from here files the report as spam.
pub const SPAM_SCORE: i64 = 3;

/// The admin's switches, in the main database's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// This instance takes reports.
    pub receive: bool,
    /// Where this instance's people send theirs.
    pub send_to: SendTo,
}

impl Settings {
    /// What the apps are told: sending here needs this instance to receive.
    pub fn info(self) -> FeedbackInfo {
        FeedbackInfo {
            receive: self.receive,
            send_to: if self.send_to == SendTo::Here && !self.receive {
                SendTo::Off
            } else {
                self.send_to
            },
        }
    }
}

/// The switches as set on the admin page, or the config's until then.
pub fn settings(conn: &Connection, config: &FeedbackConfig) -> Result<Settings, DbError> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'feedback'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(text
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Settings {
            receive: config.receive,
            send_to: config.send_to,
        }))
}

/// Saves the admin's switches.
pub fn set_settings(conn: &Connection, settings: Settings) -> Result<(), DbError> {
    let text = serde_json::to_string(&settings).unwrap_or_default();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('feedback', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [text],
    )?;
    Ok(())
}

/// Caps against floods set on the admin page, each over the config's own
/// when set. Kept apart from the switches: the page saves those alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Caps {
    /// Reports an hour from one server's people, or one device of an app
    /// on no server.
    #[serde(default)]
    pub per_hour: Option<u32>,
    /// Reports a day, counted the same way.
    #[serde(default)]
    pub per_day: Option<u32>,
}

/// The highest cap the admin page takes.
pub const MAX_CAP: u32 = 100_000;

impl Caps {
    /// The config with these caps over it.
    pub fn over(self, config: &FeedbackConfig) -> FeedbackConfig {
        FeedbackConfig {
            per_hour: self.per_hour.unwrap_or(config.per_hour),
            per_day: self.per_day.unwrap_or(config.per_day),
            ..config.clone()
        }
    }

    /// The config's own, for the page to show what an empty field means.
    pub fn of(config: &FeedbackConfig) -> Self {
        Self {
            per_hour: Some(config.per_hour),
            per_day: Some(config.per_day),
        }
    }

    /// Each cap set is between 1 and [`MAX_CAP`].
    pub fn valid(self) -> bool {
        [self.per_hour, self.per_day]
            .into_iter()
            .flatten()
            .all(|cap| (1..=MAX_CAP).contains(&cap))
    }
}

/// The caps as set on the admin page, none until then.
pub fn caps(conn: &Connection) -> Result<Caps, DbError> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'feedback_caps'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(text
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default())
}

/// Saves the admin's caps.
pub fn set_caps(conn: &Connection, caps: Caps) -> Result<(), DbError> {
    let text = serde_json::to_string(&caps).unwrap_or_default();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('feedback_caps', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [text],
    )?;
    Ok(())
}

/// The feedback database's own random key, made on first use: it seals
/// challenges and hashes addresses.
fn secret(conn: &Connection) -> Result<Vec<u8>, DbError> {
    let found: Option<Vec<u8>> = conn
        .query_row("SELECT value FROM meta WHERE key = 'secret'", [], |r| {
            r.get(0)
        })
        .optional()?;
    if let Some(secret) = found {
        return Ok(secret);
    }
    let mut secret = vec![0_u8; 32];
    getrandom::fill(&mut secret).expect("the system random source works");
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('secret', ?1)",
        [&secret],
    )?;
    Ok(secret)
}

fn mac(secret: &[u8], label: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac takes any key length");
    mac.update(label);
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// The address as kept: a keyed hash of it, of its first 64 bits for IPv6
/// (one home or one server has a whole /64 to move around in).
pub fn address_key(conn: &Connection, ip: IpAddr) -> Result<String, DbError> {
    let secret = secret(conn)?;
    let prefix = match ip {
        IpAddr::V4(v4) => v4.octets().to_vec(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.octets().to_vec(),
            None => v6.octets()[..8].to_vec(),
        },
    };
    let hash = mac(&secret, b"feedback address\0", &[&prefix]);
    Ok(hex(&hash[..16]))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Reports taken in from `address` since `since`.
fn sends_from(conn: &Connection, address: &str, since: i64) -> Result<i64, DbError> {
    Ok(conn.query_row(
        "SELECT count(*) FROM sends WHERE address = ?1 AND at > ?2",
        params![address, since],
        |r| r.get(0),
    )?)
}

/// A new challenge for `address`, harder the more it sent in the last hour.
pub fn challenge(
    conn: &Connection,
    now: i64,
    address: &str,
    config: &FeedbackConfig,
) -> Result<Challenge, DbError> {
    let recent = sends_from(conn, address, now - HOUR)?;
    let ceiling = MAX_BUSY_BITS.max(config.pow_bits);
    let extra = u8::try_from(recent.saturating_mul(2)).unwrap_or(u8::MAX);
    let bits = config.pow_bits.saturating_add(extra).min(ceiling);
    let mut id = [0_u8; 16];
    getrandom::fill(&mut id).expect("the system random source works");
    let expires_at = now + CHALLENGE_LIFE;
    let mut token = Vec::with_capacity(41);
    token.extend(id);
    token.extend(expires_at.to_be_bytes());
    token.push(bits);
    let seal = mac(&secret(conn)?, b"feedback challenge\0", &[&token]);
    token.extend(&seal[..16]);
    Ok(Challenge {
        challenge: URL_SAFE_NO_PAD.encode(token),
        bits,
        expires_at,
    })
}

/// Why a report's challenge is not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeRefused {
    /// Not one this server made.
    Unknown,
    /// Too old.
    Expired,
    /// Answered before.
    Used,
    /// The nonce does not answer it.
    NotSolved,
}

impl ChallengeRefused {
    /// In words, for the api's message.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "this challenge is not from this server",
            Self::Expired => "the challenge expired, ask for a new one",
            Self::Used => "the challenge was used already, ask for a new one",
            Self::NotSolved => "the nonce does not answer the challenge",
        }
    }
}

/// Checks the challenge and its answer, and marks it used.
pub fn take_challenge(
    conn: &Connection,
    now: i64,
    challenge: &str,
    nonce: u64,
) -> Result<Result<(), ChallengeRefused>, DbError> {
    let Ok(token) = URL_SAFE_NO_PAD.decode(challenge) else {
        return Ok(Err(ChallengeRefused::Unknown));
    };
    if token.len() != 41 {
        return Ok(Err(ChallengeRefused::Unknown));
    }
    let (body, seal) = token.split_at(25);
    let expected = mac(&secret(conn)?, b"feedback challenge\0", &[body]);
    // compared in constant time, the seal is what keeps challenges ours
    let differs = expected[..16]
        .iter()
        .zip(seal)
        .fold(0_u8, |acc, (a, b)| acc | (a ^ b));
    if differs != 0 {
        return Ok(Err(ChallengeRefused::Unknown));
    }
    let id = &body[..16];
    let mut expires = [0_u8; 8];
    expires.copy_from_slice(&body[16..24]);
    let expires_at = i64::from_be_bytes(expires);
    let bits = body[24];
    if expires_at <= now {
        return Ok(Err(ChallengeRefused::Expired));
    }
    if !app_protocol::feedback::meets(challenge, nonce, bits) {
        return Ok(Err(ChallengeRefused::NotSolved));
    }
    let added = conn.execute(
        "INSERT OR IGNORE INTO used_challenges (id, expires_at) VALUES (?1, ?2)",
        params![id, expires_at],
    )?;
    if added == 0 {
        return Ok(Err(ChallengeRefused::Used));
    }
    Ok(Ok(()))
}

/// The seconds to wait when a limit is reached.
fn wait_in_window(
    conn: &Connection,
    sql: &str,
    key: &str,
    now: i64,
    window: i64,
    limit: u32,
) -> Result<Option<u64>, DbError> {
    let (count, oldest): (i64, Option<i64>) =
        conn.query_row(sql, params![key, now - window], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    if count < i64::from(limit) {
        return Ok(None);
    }
    let until = oldest.unwrap_or(now) + window;
    Ok(Some(seconds_until(now, until)))
}

fn seconds_until(now: i64, until: i64) -> u64 {
    u64::try_from((until - now).max(0) / 1000 + 1).unwrap_or(1)
}

/// The whole instance's reports today, the seconds until tomorrow when full.
fn instance_full(
    conn: &Connection,
    now: i64,
    config: &FeedbackConfig,
) -> Result<Option<u64>, DbError> {
    let day_start = now - now.rem_euclid(DAY);
    let today: i64 = conn.query_row(
        "SELECT count(*) FROM sends WHERE at >= ?1",
        [day_start],
        |r| r.get(0),
    )?;
    Ok((today >= i64::from(config.instance_per_day)).then(|| seconds_until(now, day_start + DAY)))
}

/// Who sends a report, as far as the limits go.
#[derive(Debug, Clone, Copy)]
pub struct Sender<'a> {
    /// The keyed hash of the address.
    pub address: &'a str,
    /// The app's install id, when sent.
    pub install: Option<&'a str>,
    /// The server the app says it is signed in to.
    pub server: Option<&'a str>,
    /// Signed in to this instance: the admin's caps do not apply.
    pub member: bool,
}

/// Reports from one address an hour and a day, whatever server it names
/// or account it has: a name in a report proves nothing.
pub const SAFETY_PER_HOUR: u32 = 60;
/// See [`SAFETY_PER_HOUR`].
pub const SAFETY_PER_DAY: u32 = 300;

/// `Err` holds the seconds before `sender` may send again. The admin's caps
/// count per server; an app on no server counts per address and install
/// ([`check_limits`]); this instance's own people are not capped. The
/// instance's total and [`SAFETY_PER_HOUR`] hold for everyone.
pub fn check_sender(
    conn: &Connection,
    now: i64,
    sender: Sender<'_>,
    config: &FeedbackConfig,
) -> Result<Result<(), u64>, DbError> {
    let Some(server) = sender.server.filter(|_| !sender.member) else {
        if !sender.member {
            return check_limits(conn, now, sender.address, sender.install, config);
        }
        return safety(conn, now, sender.address, config);
    };
    if let Err(wait) = safety(conn, now, sender.address, config)? {
        return Ok(Err(wait));
    }
    let by_server = "SELECT count(*), min(at) FROM sends WHERE server = ?1 AND at > ?2";
    for (window, limit) in [(HOUR, config.per_hour), (DAY, config.per_day)] {
        if let Some(wait) = wait_in_window(conn, by_server, server, now, window, limit)? {
            return Ok(Err(wait));
        }
    }
    Ok(Ok(()))
}

/// The instance's total and the per address safety.
fn safety(
    conn: &Connection,
    now: i64,
    address: &str,
    config: &FeedbackConfig,
) -> Result<Result<(), u64>, DbError> {
    if let Some(wait) = instance_full(conn, now, config)? {
        return Ok(Err(wait));
    }
    let by_address = "SELECT count(*), min(at) FROM sends WHERE address = ?1 AND at > ?2";
    for (window, limit) in [(HOUR, SAFETY_PER_HOUR), (DAY, SAFETY_PER_DAY)] {
        if let Some(wait) = wait_in_window(conn, by_address, address, now, window, limit)? {
            return Ok(Err(wait));
        }
    }
    Ok(Ok(()))
}

/// `Err` holds the seconds before `address` (and `install`) may send again.
pub fn check_limits(
    conn: &Connection,
    now: i64,
    address: &str,
    install: Option<&str>,
    config: &FeedbackConfig,
) -> Result<Result<(), u64>, DbError> {
    if let Some(wait) = instance_full(conn, now, config)? {
        return Ok(Err(wait));
    }
    // without an install id, every report of the address is a stranger's
    let (per_hour, per_day) = match install {
        Some(_) => (config.per_hour, config.per_day),
        None => ((config.per_hour / 2).max(1), (config.per_day / 2).max(1)),
    };
    let by_address = "SELECT count(*), min(at) FROM sends WHERE address = ?1 AND at > ?2";
    let by_install = "SELECT count(*), min(at) FROM sends WHERE install = ?1 AND at > ?2";
    let mut checks = vec![
        (by_address, address, HOUR, per_hour),
        (by_address, address, DAY, per_day),
    ];
    if let Some(install) = install {
        checks.push((by_install, install, HOUR, config.per_hour));
        checks.push((by_install, install, DAY, config.per_day));
    }
    for (sql, key, window, limit) in checks {
        if let Some(wait) = wait_in_window(conn, sql, key, now, window, limit)? {
            return Ok(Err(wait));
        }
    }
    Ok(Ok(()))
}

/// The server a report names, lowercased: a host name, maybe with a port.
/// The caps count by it, nothing else.
fn server_host(server: Option<String>) -> Result<Option<String>, String> {
    let Some(server) = server
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let host = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']');
    if server.len() > 255 || !server.bytes().all(host) {
        return Err("the server is not a host name".into());
    }
    Ok(Some(server))
}

/// A report that passed every check of form and size.
#[derive(Debug)]
pub struct Incoming {
    /// What it is about, from apps before 0.2.0.
    pub category: Option<Category>,
    /// What it is about in one line, trimmed, from apps since 0.2.0.
    pub about: Option<String>,
    /// The words, trimmed, empty when `about` says it all.
    pub message: String,
    /// An email address or a Discord name, lowercased, when given.
    pub contact: Option<String>,
    /// The app's version.
    pub version: String,
    /// The app's platform.
    pub platform: String,
    /// When attached.
    pub logs: Option<String>,
    /// The install id, when sent.
    pub install: Option<String>,
    /// The host of the server the app is signed in to, lowercased.
    pub server: Option<String>,
    /// The receipt, 64 lowercase hex, when sent.
    pub receipt: Option<String>,
    /// Each image with what it is.
    pub images: Vec<(images::ImageInfo, Vec<u8>)>,
}

/// A receipt as sent: 64 lowercase hex, a SHA-256.
fn receipt(sent: Option<String>) -> Result<Option<String>, String> {
    match sent {
        None => Ok(None),
        Some(hash)
            if hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) =>
        {
            Ok(Some(hash))
        }
        Some(_) => Err("the receipt is not 64 lowercase hex".into()),
    }
}

fn control(c: char) -> bool {
    c.is_control() && !matches!(c, '\n' | '\r' | '\t')
}

fn plain(text: &str, max: usize, allowed: impl Fn(char) -> bool) -> bool {
    !text.is_empty() && text.chars().count() <= max && text.chars().all(allowed)
}

/// An email address, or a Discord name: today's (2 to 32 of `a-z 0-9 _ .`,
/// an `@` before it is dropped) or an older one with its `#1234`.
fn contact(text: &str) -> Option<String> {
    if text.len() > MAX_CONTACT || text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let text = text.to_lowercase();
    let name = text.strip_prefix('@').unwrap_or(&text);
    if let Some((local, domain)) = name.split_once('@') {
        let shaped = !local.is_empty() && domain.contains('.') && !domain.contains('@');
        return (shaped && name == text).then_some(text);
    }
    let today = (2..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.".contains(&b));
    let older = name.rsplit_once('#').is_some_and(|(user, tag)| {
        (2..=32).contains(&user.chars().count())
            && !user.contains(['@', '#', ':'])
            && tag.len() == 4
            && tag.bytes().all(|b| b.is_ascii_digit())
    });
    (today || older).then(|| name.to_owned())
}

/// Checks a report's form and sizes. The message says what is wrong.
pub fn validate(report: Report) -> Result<Incoming, String> {
    let about = report
        .about
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty());
    if let Some(about) = about {
        if about.chars().count() > MAX_ABOUT {
            return Err(format!("the about line is over {MAX_ABOUT} characters"));
        }
        if about.chars().any(char::is_control) {
            return Err("the about line holds control characters".into());
        }
    }
    let message = report.message.trim().to_owned();
    if message.is_empty() && about.is_none() {
        return Err("the message is empty".into());
    }
    if message.chars().count() > MAX_MESSAGE {
        return Err(format!("the message is over {MAX_MESSAGE} characters"));
    }
    if message.chars().any(control) {
        return Err("the message holds control characters".into());
    }
    if !plain(&report.version, 64, |c| {
        c.is_ascii_alphanumeric() || ".+-_ ".contains(c)
    }) {
        return Err("the version is not a version".into());
    }
    if !plain(&report.platform, 32, |c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || "-_".contains(c)
    }) {
        return Err("the platform is not a platform".into());
    }
    let contact = match report.contact.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(text) => {
            Some(contact(text).ok_or("the contact is not an email address or a Discord name")?)
        }
    };
    let logs = match report.logs {
        None => None,
        Some(logs) if logs.trim().is_empty() => None,
        Some(logs) => {
            if logs.len() > MAX_LOGS {
                return Err(format!("the logs are over {} KB", MAX_LOGS / 1024));
            }
            if logs.contains('\0') {
                return Err("the logs hold a NUL".into());
            }
            Some(logs)
        }
    };
    let install = match report.install {
        None => None,
        Some(id)
            if id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) =>
        {
            Some(id)
        }
        Some(_) => return Err("the install id is not 32 lowercase hex".into()),
    };
    let server = server_host(report.server)?;
    let receipt = receipt(report.receipt)?;
    if report.images.len() > MAX_IMAGES {
        return Err(format!("more than {MAX_IMAGES} images"));
    }
    let mut taken = Vec::new();
    for (n, encoded) in report.images.iter().enumerate() {
        // checked before decoding, a huge string is never decoded
        if encoded.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
            return Err(format!("image {} is over 2 MB", n + 1));
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| format!("image {} is not base64", n + 1))?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(format!("image {} is over 2 MB", n + 1));
        }
        let info = images::check(&bytes).map_err(|why| format!("image {}: {why}", n + 1))?;
        taken.push((info, bytes));
    }
    Ok(Incoming {
        category: report.category,
        about: about.map(str::to_owned),
        message,
        contact,
        version: report.version,
        platform: report.platform,
        logs,
        install,
        server,
        receipt,
        images: taken,
    })
}

/// What happened to a report taken in. The sender is told the same for all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// In the inbox.
    Saved(String),
    /// In the spam folder.
    Spam(String),
    /// From a blocked sender, not kept.
    Dropped,
}

fn message_hash(message: &str) -> String {
    let normal = message
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    hex(&Sha256::digest(normal.as_bytes())[..16])
}

fn blocked(
    conn: &Connection,
    now: i64,
    address: &str,
    incoming: &Incoming,
) -> Result<bool, DbError> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM blocks
             WHERE (expires_at IS NULL OR expires_at > ?1)
               AND ((kind = 'address' AND value = ?2)
                 OR (kind = 'install' AND value = ?3)
                 OR (kind = 'email' AND value = ?4))
             LIMIT 1",
            params![now, address, incoming.install, incoming.contact],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

/// The spam score and what made it.
pub fn spam_score(
    conn: &Connection,
    now: i64,
    message: &str,
    hash: &str,
) -> Result<(i64, Vec<&'static str>), DbError> {
    let mut score = 0;
    let mut flags = Vec::new();
    let lower = message.to_lowercase();
    let links = ["http://", "https://", "www."]
        .iter()
        .map(|p| lower.matches(p).count())
        .sum::<usize>();
    if links >= 3 {
        score += 3;
        flags.push("links");
    } else if links > 0 {
        score += 1;
        flags.push("link");
    }
    let repeated: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sends WHERE message_hash = ?1 AND at > ?2 LIMIT 1",
            params![hash, now - DAY],
            |r| r.get(0),
        )
        .optional()?;
    if repeated.is_some() {
        score += 3;
        flags.push("repeated");
    }
    let mut words = conn.prepare(
        "SELECT value FROM blocks WHERE kind = 'word' AND (expires_at IS NULL OR expires_at > ?1)",
    )?;
    let words = words
        .query_map([now], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if words.iter().any(|w| lower.contains(&w.to_lowercase())) {
        score += 5;
        flags.push("blockedWord");
    }
    let visible: Vec<char> = message.chars().filter(|c| !c.is_whitespace()).collect();
    let letters = visible.iter().filter(|c| c.is_alphabetic()).count();
    if letters * 2 < visible.len() {
        score += 2;
        flags.push("symbols");
    }
    if message.chars().count() < 10 {
        score += 2;
        flags.push("short");
    }
    Ok((score, flags))
}

/// Takes in a checked report from `address`: dropped when its sender is
/// blocked, filed as spam when it scores so, saved otherwise. Every one
/// counts against the limits.
pub fn take(
    conn: &mut Connection,
    now: i64,
    address: &str,
    incoming: &Incoming,
) -> Result<Outcome, DbError> {
    let tx = conn.transaction()?;
    // the about line and the words are judged together
    let text = match &incoming.about {
        Some(about) => format!(
            "{about}
{}",
            incoming.message
        ),
        None => incoming.message.clone(),
    };
    let hash = message_hash(&text);
    let outcome = if blocked(&tx, now, address, incoming)? {
        Outcome::Dropped
    } else {
        let (score, flags) = spam_score(&tx, now, text.trim(), &hash)?;
        let id = uuid::Uuid::now_v7().simple().to_string();
        let spam = score >= SPAM_SCORE;
        tx.execute(
            "INSERT INTO reports (id, created_at, status, category, about, message, contact,
                 version, platform, logs, install, address, score, flags, receipt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                id,
                now,
                if spam { "spam" } else { "new" },
                // no category from apps since 0.2.0
                incoming.category.map_or("", Category::as_str),
                incoming.about,
                incoming.message,
                incoming.contact,
                incoming.version,
                incoming.platform,
                incoming.logs,
                incoming.install,
                address,
                score,
                flags.join(","),
                incoming.receipt,
            ],
        )?;
        for (position, (info, bytes)) in incoming.images.iter().enumerate() {
            tx.execute(
                "INSERT INTO images (report, position, kind, width, height, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    i64::try_from(position).unwrap_or(0),
                    info.kind,
                    info.width,
                    info.height,
                    bytes
                ],
            )?;
        }
        if spam {
            Outcome::Spam(id)
        } else {
            Outcome::Saved(id)
        }
    };
    tx.execute(
        "INSERT INTO sends (at, address, install, message_hash, server) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![now, address, incoming.install, hash, incoming.server],
    )?;
    tx.commit()?;
    Ok(outcome)
}

/// What a block is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockKind {
    /// One install of the app.
    Install,
    /// One address (its keyed hash).
    Address,
    /// One contact, an email address or a Discord name (`email`, its name
    /// from before 0.2.0).
    Email,
    /// A word or phrase that makes a report spam.
    Word,
}

impl BlockKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Address => "address",
            Self::Email => "email",
            Self::Word => "word",
        }
    }
}

/// Adds or renews a block. `expires_at` `None` never ends.
pub fn block(
    conn: &Connection,
    now: i64,
    kind: BlockKind,
    value: &str,
    note: &str,
    expires_at: Option<i64>,
) -> Result<(), DbError> {
    let value = match kind {
        BlockKind::Email | BlockKind::Word => value.trim().to_lowercase(),
        BlockKind::Install | BlockKind::Address => value.trim().to_owned(),
    };
    conn.execute(
        "INSERT INTO blocks (kind, value, note, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (kind, value) DO UPDATE
         SET note = excluded.note, created_at = excluded.created_at,
             expires_at = excluded.expires_at",
        params![kind.as_str(), value, note, now, expires_at],
    )?;
    Ok(())
}

/// Removes what has had its time: reports past `keep_days` and not kept,
/// sends after a day, expired challenges and blocks. Returns how many rows.
pub fn cleanup(conn: &Connection, now: i64, keep_days: u32) -> Result<usize, DbError> {
    let mut removed = conn.execute(
        "DELETE FROM reports WHERE kept = 0 AND created_at < ?1",
        [now - i64::from(keep_days) * DAY],
    )?;
    removed += conn.execute("DELETE FROM sends WHERE at < ?1", [now - DAY])?;
    removed += conn.execute("DELETE FROM used_challenges WHERE expires_at <= ?1", [now])?;
    removed += conn.execute(
        "DELETE FROM blocks WHERE expires_at IS NOT NULL AND expires_at <= ?1",
        [now],
    )?;
    Ok(removed)
}

#[cfg(test)]
mod tests;

/// How the reports of these receipts' secrets stand, for their sender. A
/// secret that matches nothing is left out.
///
/// # Errors
///
/// Database errors only.
pub fn answers(
    conn: &Connection,
    secrets: &[String],
) -> Result<Vec<app_protocol::feedback::SentReport>, DbError> {
    use app_protocol::feedback::{SentReport, SentState};
    use std::fmt::Write;

    use sha2::{Digest, Sha256};
    let mut out = Vec::new();
    let mut stmt =
        conn.prepare_cached("SELECT status, answer FROM reports WHERE receipt = ?1 LIMIT 1")?;
    for secret in secrets {
        let mut receipt = String::with_capacity(64);
        for byte in Sha256::digest(secret.as_bytes()) {
            let _ = write!(receipt, "{byte:02x}");
        }
        let found: Option<(String, String)> = stmt
            .query_row([&receipt], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        if let Some((status, answer)) = found {
            let state = match status.as_str() {
                "open" => SentState::Open,
                "done" => SentState::Done,
                // spam reads as new: nothing of the filters shows
                _ => SentState::Received,
            };
            out.push(SentReport {
                secret: secret.clone(),
                state,
                answer,
            });
        }
    }
    Ok(out)
}
