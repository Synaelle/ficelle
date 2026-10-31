// SPDX-License-Identifier: AGPL-3.0-or-later

//! Update notices: once a day the server fetches the signed release list
//! (`[updates] url`, the brand's by default), keeps the entries signed with
//! `[updates] public_key`, and tells the admin page whether a newer
//! release, or a security one, exists. Nothing is installed: the admin
//! updates the image or the binary. The admin page can turn the check off,
//! and `[updates] check = false` keeps the server from ever asking.

use std::time::Duration;

use app_protocol::catalog::hex_bytes;
use app_protocol::releases::{
    FORMAT, Release, ReleaseList, UpdatesView, signed_message, version_parts,
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::config::UpdatesConfig;
use crate::db::DbError;
use crate::routes::AppState;

/// `releases.json`, at most.
const MAX_LIST: usize = 1024 * 1024;
/// Between two checks.
const EVERY: i64 = 24 * 3_600_000;

/// Whether the admin lets the server check. On until turned off.
///
/// # Errors
///
/// Database errors.
pub fn enabled(conn: &Connection) -> Result<bool, DbError> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'updates'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.as_deref() != Some("off"))
}

/// The admin's switch.
///
/// # Errors
///
/// Database errors.
pub fn set_enabled(conn: &Connection, on: bool) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('updates', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [if on { "on" } else { "off" }],
    )?;
    Ok(())
}

/// Whether a release was signed with `public_key` and names a version.
pub fn release_signed(release: &Release, public_key: &str) -> bool {
    let key = hex_bytes::<32>(public_key).and_then(|k| VerifyingKey::from_bytes(&k).ok());
    let (Some(key), Some(signature)) = (key, hex_bytes::<64>(&release.signature)) else {
        return false;
    };
    version_parts(&release.version).is_some()
        && key
            .verify(&signed_message(release), &Signature::from_bytes(&signature))
            .is_ok()
}

/// What a list says about this version: the newest release, whether it is
/// newer, whether a newer one fixes a security problem.
pub fn compare(releases: &[Release], current: &str) -> (Option<Release>, bool, bool) {
    let latest = releases
        .iter()
        .max_by_key(|r| version_parts(&r.version))
        .cloned();
    let Some(mine) = version_parts(current) else {
        // a dev or preview build: the newest is shown, nothing is flagged
        return (latest, false, false);
    };
    let newer: Vec<&Release> = releases
        .iter()
        .filter(|r| version_parts(&r.version).is_some_and(|v| v > mine))
        .collect();
    let security = newer.iter().any(|r| r.security);
    (latest, !newer.is_empty(), security)
}

#[derive(Default)]
struct Held {
    releases: Vec<Release>,
    checked_at: Option<i64>,
    error: Option<String>,
}

/// The release list as last fetched.
pub struct Updates {
    config: UpdatesConfig,
    held: Mutex<Held>,
    http: reqwest::Client,
}

impl Updates {
    /// A checker for `config`.
    pub fn new(config: UpdatesConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            // the name alone: an outside host has no need of the version
            .user_agent(app_brand::SLUG)
            .build()
            .unwrap_or_default();
        Self {
            config,
            held: Mutex::default(),
            http,
        }
    }

    /// The server may check at all: allowed by its config, with a key.
    pub fn available(&self) -> bool {
        self.config.check && !self.config.public_key.is_empty() && !self.config.url.is_empty()
    }

    async fn fetch(&self) -> Result<Vec<Release>, String> {
        let mut response = self
            .http
            .get(&self.config.url)
            .send()
            .await
            .map_err(|e| format!("cannot reach the release list: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("the release list answered {}", response.status()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MAX_LIST {
                return Err("the release list is larger than it may be".into());
            }
        }
        let list: ReleaseList =
            serde_json::from_slice(&bytes).map_err(|e| format!("releases.json: {e}"))?;
        if list.format != FORMAT {
            return Err(format!(
                "release list format {} is not read here",
                list.format
            ));
        }
        let total = list.releases.len();
        let signed: Vec<Release> = list
            .releases
            .into_iter()
            .filter(|r| release_signed(r, &self.config.public_key))
            .collect();
        if signed.len() < total {
            tracing::warn!(
                "the release list has {} entries not signed with the key, ignored",
                total - signed.len()
            );
        }
        Ok(signed)
    }

    /// Fetches the list again. The last good one stays when it fails.
    pub async fn check(&self, now: i64) {
        let fetched = self.fetch().await;
        let mut held = self.held.lock().await;
        match fetched {
            Ok(releases) => {
                held.releases = releases;
                held.error = None;
            }
            Err(why) => {
                tracing::info!("update check: {why}");
                held.error = Some(why);
            }
        }
        held.checked_at = Some(now);
    }

    /// Whether a day went by since the last check.
    pub async fn due(&self, now: i64) -> bool {
        self.held
            .lock()
            .await
            .checked_at
            .is_none_or(|at| now - at >= EVERY)
    }

    /// What the admin page shows.
    pub async fn view(&self, enabled: bool) -> UpdatesView {
        let held = self.held.lock().await;
        let (latest, newer, security) = compare(&held.releases, app_brand::VERSION);
        UpdatesView {
            enabled,
            available: self.available(),
            current: app_brand::VERSION.to_owned(),
            latest,
            newer,
            security,
            checked_at: held.checked_at,
            error: held.error.clone(),
        }
    }
}

/// Checks once a day while the config and the admin allow it. The first
/// check waits a minute after the start, so a crash loop does not hammer
/// the list's host.
pub fn spawn(state: AppState) {
    if !state.updates.available() {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let mut every = tokio::time::interval(Duration::from_secs(3600));
        loop {
            every.tick().await;
            let now = state.clock.now();
            let on = state.db.call(|c| enabled(c)).await.unwrap_or(false);
            if on && state.updates.due(now).await {
                state.updates.check(now).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut out, b| {
            use std::fmt::Write as _;
            let _ = write!(out, "{b:02x}");
            out
        })
    }

    pub(crate) fn release(version: &str, security: bool, key: &SigningKey) -> Release {
        let mut release = Release {
            version: version.into(),
            date: "2026-11-01".into(),
            security,
            notes: format!("release {version}"),
            url: String::new(),
            signature: String::new(),
        };
        release.signature = hex(&key.sign(&signed_message(&release)).to_bytes());
        release
    }

    #[test]
    fn only_entries_signed_with_the_key_count() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = hex(key.verifying_key().as_bytes());
        let good = release("0.2.0", false, &key);
        assert!(release_signed(&good, &public));
        let mut changed = good.clone();
        changed.security = true;
        assert!(
            !release_signed(&changed, &public),
            "a field changed after signing"
        );
        let other = SigningKey::from_bytes(&[8; 32]);
        assert!(!release_signed(&release("0.2.0", false, &other), &public));
        assert!(!release_signed(&release("preview", false, &key), &public));
        assert!(!release_signed(&good, ""));
    }

    #[test]
    fn newer_and_security_releases_are_flagged() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let list = vec![
            release("0.1.0", false, &key),
            release("0.1.1", true, &key),
            release("0.2.0", false, &key),
        ];
        let (latest, newer, security) = compare(&list, "0.1.0");
        assert_eq!(latest.unwrap().version, "0.2.0");
        assert!(newer && security);
        let (_, newer, security) = compare(&list, "0.1.1");
        assert!(newer && !security, "the security fix is in");
        let (_, newer, _) = compare(&list, "0.2.0");
        assert!(!newer);
        let (latest, newer, security) = compare(&list, "dev-abc");
        assert!(latest.is_some() && !newer && !security);
        assert_eq!(compare(&[], "0.1.0"), (None, false, false));
    }
}
