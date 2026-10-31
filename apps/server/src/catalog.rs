// SPDX-License-Identifier: AGPL-3.0-or-later

//! The official catalog for this instance's apps (see
//! `app_protocol::catalog`). Off until the admin turns it on. Then the
//! server fetches `catalog.json` from `[catalog] url`, keeps only the
//! entries signed with `[catalog] public_key`, and serves them; the files
//! are fetched when an app asks, checked against their signed hash and kept
//! on disk by it. The apps check everything again: this cache only spares
//! the catalog's host and lets apps reach it through their own server.

use std::path::PathBuf;
use std::time::Duration;

use app_protocol::catalog::{
    Catalog, CatalogEntry, CatalogKind, CatalogView, FORMAT, hex_bytes, safe_file, signed_message,
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::config::CatalogConfig;
use crate::db::DbError;

/// `catalog.json`, at most.
const MAX_INDEX: usize = 5 * 1024 * 1024;
/// A theme file, at most.
const MAX_THEME: usize = 2 * 1024 * 1024;

/// Whether the admin turned the catalog on.
///
/// # Errors
///
/// Database errors.
pub fn enabled(conn: &Connection) -> Result<bool, DbError> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'catalog'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.as_deref() == Some("on"))
}

/// Turns the catalog on or off for the instance.
///
/// # Errors
///
/// Database errors.
pub fn set_enabled(conn: &Connection, on: bool) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('catalog', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [if on { "on" } else { "off" }],
    )?;
    Ok(())
}

/// Whether an entry was signed with `public_key` and names a file inside
/// the catalog. The apps run the same check (`app_core::catalog`).
pub fn entry_signed(entry: &CatalogEntry, public_key: &str) -> bool {
    let key = hex_bytes::<32>(public_key).and_then(|k| VerifyingKey::from_bytes(&k).ok());
    let (Some(key), Some(signature)) = (key, hex_bytes::<64>(&entry.signature)) else {
        return false;
    };
    safe_file(&entry.file)
        && key
            .verify(&signed_message(entry), &Signature::from_bytes(&signature))
            .is_ok()
}

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, b| {
            // writing to a String cannot fail
            let _ = write!(out, "{b:02x}");
            out
        })
}

#[derive(Default)]
struct Held {
    catalog: Option<Catalog>,
    fetched_at: Option<i64>,
    error: Option<String>,
}

/// The catalog as last fetched, and where its files are kept.
pub struct Cache {
    config: CatalogConfig,
    files: PathBuf,
    held: Mutex<Held>,
    http: reqwest::Client,
}

impl Cache {
    /// A cache for `config`, files kept under `data_dir/catalog`.
    pub fn new(config: CatalogConfig, data_dir: &std::path::Path) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            // the name alone: an outside host has no need of the version
            .user_agent(app_brand::SLUG)
            .build()
            .unwrap_or_default();
        Self {
            config,
            files: data_dir.join("catalog"),
            held: Mutex::default(),
            http,
        }
    }

    async fn get(&self, path: &str, max: usize) -> Result<Vec<u8>, String> {
        let url = format!("{}{path}", self.config.url);
        let mut response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("cannot reach the catalog: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("the catalog answered {}", response.status()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > max {
                return Err("the catalog sent more than it may".into());
            }
        }
        Ok(bytes)
    }

    async fn fetch(&self) -> Result<Catalog, String> {
        if self.config.public_key.is_empty() {
            return Err("no catalog key is set".into());
        }
        let bytes = self.get("catalog.json", MAX_INDEX).await?;
        let mut catalog: Catalog =
            serde_json::from_slice(&bytes).map_err(|e| format!("catalog.json: {e}"))?;
        if catalog.format != FORMAT {
            return Err(format!(
                "catalog format {} is not read here",
                catalog.format
            ));
        }
        catalog
            .entries
            .retain(|e| entry_signed(e, &self.config.public_key));
        Ok(catalog)
    }

    /// The catalog for `enabled`, fetched again when it is older than the
    /// refresh time or when `force`.
    pub async fn view(&self, enabled: bool, now: i64, force: bool) -> CatalogView {
        if !enabled {
            return CatalogView::default();
        }
        let mut held = self.held.lock().await;
        let refresh = i64::from(self.config.refresh_hours) * 3_600_000;
        let stale = held.fetched_at.is_none_or(|at| now - at >= refresh);
        if force || stale {
            match self.fetch().await {
                Ok(catalog) => {
                    held.catalog = Some(catalog);
                    held.error = None;
                }
                // the last good catalog stays, with why it was not renewed
                Err(why) => held.error = Some(why),
            }
            held.fetched_at = Some(now);
        }
        CatalogView {
            enabled,
            catalog: held.catalog.clone(),
            error: held.error.clone(),
            fetched_at: held.fetched_at,
        }
    }

    /// A file of the catalog, fetched once and kept by its hash, always
    /// checked against the signed entry. `None` when the catalog has no
    /// such entry.
    ///
    /// # Errors
    ///
    /// Why the file could not be had.
    pub async fn file(&self, kind: CatalogKind, id: &str) -> Result<Option<Vec<u8>>, String> {
        let entry = {
            let held = self.held.lock().await;
            held.catalog.as_ref().and_then(|c| {
                c.entries
                    .iter()
                    .find(|e| e.kind == kind && e.id == id)
                    .cloned()
            })
        };
        let Some(entry) = entry else { return Ok(None) };
        if !entry_signed(&entry, &self.config.public_key)
            || hex_bytes::<32>(&entry.file_sha256).is_none()
        {
            return Ok(None);
        }
        let kept = self.files.join(&entry.file_sha256);
        if let Ok(bytes) = tokio::fs::read(&kept).await
            && hex_sha256(&bytes) == entry.file_sha256
        {
            return Ok(Some(bytes));
        }
        let bytes = self.get(&entry.file, MAX_THEME).await?;
        if hex_sha256(&bytes) != entry.file_sha256 {
            return Err("the file is not the one the catalog signed".into());
        }
        // keeping a copy only spares the next fetch
        if tokio::fs::create_dir_all(&self.files).await.is_ok() {
            let _ = tokio::fs::write(&kept, &bytes).await;
        }
        Ok(Some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn entry() -> CatalogEntry {
        CatalogEntry {
            kind: CatalogKind::Theme,
            id: "ocean".into(),
            name: "Ocean".into(),
            version: "abc".into(),
            description: None,
            author: None,
            file: "themes/ocean.json".into(),
            size: 2,
            file_sha256: hex_sha256(b"{}"),
            content_sha256: hex_sha256(b"{}"),
            signature: String::new(),
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut out, b| {
            use std::fmt::Write as _;
            // writing to a String cannot fail
            let _ = write!(out, "{b:02x}");
            out
        })
    }

    #[test]
    fn entries_check_as_the_apps_check_them() {
        let key = SigningKey::from_bytes(&[3; 32]);
        let public = hex(key.verifying_key().as_bytes());
        let mut signed = entry();
        signed.signature = hex(&key.sign(&signed_message(&signed)).to_bytes());
        assert!(entry_signed(&signed, &public));
        let mut changed = signed.clone();
        changed.file_sha256 = hex_sha256(b"other");
        assert!(!entry_signed(&changed, &public));
        let other = hex(SigningKey::from_bytes(&[4; 32]).verifying_key().as_bytes());
        assert!(!entry_signed(&signed, &other));
        assert!(!entry_signed(&signed, ""));
    }
}
