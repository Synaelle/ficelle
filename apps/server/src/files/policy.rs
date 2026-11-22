// SPDX-License-Identifier: AGPL-3.0-or-later

//! The rules for files, and the type checks.
//!
//! Each key is looked up on its own: the instance settings from the admin
//! page, else `[files]` in the config file, whose defaults need nothing set.

use std::collections::BTreeMap;

use app_protocol::admin::{PolicySettings, PolicySource};
use app_protocol::files::FilesPolicy;
use rusqlite::{Connection, OptionalExtension, params};

use crate::config::FilesConfig;
use crate::db::DbError;

const MB: u64 = 1024 * 1024;

/// The instance settings, nothing set when there are none.
///
/// # Errors
///
/// Database errors, stored json that does not read.
pub fn settings(conn: &Connection) -> Result<PolicySettings, DbError> {
    let text: Option<String> = conn
        .query_row(
            "SELECT settings FROM policies WHERE scope = 'instance' AND target = ''",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match text {
        Some(text) => serde_json::from_str(&text).map_err(|e| DbError::Data(e.to_string())),
        None => Ok(PolicySettings::default()),
    }
}

/// Stores the instance settings, or forgets them when nothing is set.
///
/// # Errors
///
/// Database errors.
pub fn set(conn: &Connection, settings: &PolicySettings, now: i64) -> Result<(), DbError> {
    if *settings == PolicySettings::default() {
        conn.execute(
            "DELETE FROM policies WHERE scope = 'instance' AND target = ''",
            [],
        )?;
        return Ok(());
    }
    let text = serde_json::to_string(settings).map_err(|e| DbError::Data(e.to_string()))?;
    conn.execute(
        "INSERT INTO policies (scope, target, settings, updated_at) VALUES ('instance', '', ?1, ?2)
         ON CONFLICT (scope, target) DO UPDATE SET settings = excluded.settings,
             updated_at = excluded.updated_at",
        params![text, now],
    )?;
    Ok(())
}

/// Why settings cannot be kept, `None` when they can.
pub fn invalid(settings: &PolicySettings) -> Option<&'static str> {
    if settings
        .image_quality
        .is_some_and(|q| !(1..=100).contains(&q))
    {
        return Some("image quality is 1 to 100");
    }
    let lists = [&settings.allowed_types, &settings.blocked_types];
    for list in lists.into_iter().flatten() {
        if list.len() > 100 {
            return Some("at most 100 types in a list");
        }
        let well_formed = |rule: &String| {
            let rule = rule.trim();
            (2..=64).contains(&rule.len())
                && (rule.starts_with('.') || rule.contains('/'))
                && !rule.contains(char::is_whitespace)
        };
        if !list.iter().all(well_formed) {
            return Some("a type is an extension like .pdf or a type like image/*");
        }
    }
    None
}

/// One key: the instance's value, else the config's.
fn layer<T>(instance: Option<T>, config: T) -> (T, PolicySource) {
    let source = |level: &str| PolicySource {
        level: level.into(),
    };
    match instance {
        Some(value) => (value, source("instance")),
        None => (config, source("config")),
    }
}

/// Notes where a key's value comes from and hands the value on.
fn take<T>(sources: &mut BTreeMap<String, PolicySource>, key: &str, found: (T, PolicySource)) -> T {
    sources.insert(key.to_owned(), found.1);
    found.0
}

/// The policy in effect, and where each key comes from.
pub fn resolve(
    config: &FilesConfig,
    instance: &PolicySettings,
) -> (FilesPolicy, BTreeMap<String, PolicySource>) {
    let mut sources = BTreeMap::new();
    let max_file_mb = take(
        &mut sources,
        "maxFileBytes",
        layer(instance.max_file_mb, config.max_file_mb),
    );
    let quota_mb = take(
        &mut sources,
        "quotaBytes",
        layer(instance.quota_mb, config.quota_mb),
    );
    let allowed_types = take(
        &mut sources,
        "allowedTypes",
        layer(instance.allowed_types.clone(), config.allowed_types.clone()),
    );
    let blocked_types = take(
        &mut sources,
        "blockedTypes",
        layer(instance.blocked_types.clone(), config.blocked_types.clone()),
    );
    let max_files_per_note = take(
        &mut sources,
        "maxFilesPerNote",
        layer(instance.max_files_per_note, config.max_files_per_note),
    );
    let image_max_px = take(
        &mut sources,
        "imageMaxPx",
        layer(instance.image_max_px, config.image_max_px),
    );
    let image_quality = take(
        &mut sources,
        "imageQuality",
        layer(instance.image_quality, config.image_quality),
    );
    let thumbnails = take(
        &mut sources,
        "thumbnails",
        layer(instance.thumbnails, config.thumbnails),
    );
    let retention_days = take(
        &mut sources,
        "retentionDays",
        layer(instance.retention_days, config.retention_days),
    );
    let policy = FilesPolicy {
        max_file_bytes: u64::from(max_file_mb) * MB,
        quota_bytes: u64::from(quota_mb) * MB,
        allowed_types,
        blocked_types,
        max_files_per_note,
        image_max_px,
        image_quality,
        thumbnails,
        retention_days,
    };
    (policy, sources)
}

/// The policy in effect.
///
/// # Errors
///
/// Database errors.
pub fn effective(conn: &Connection, config: &FilesConfig) -> Result<FilesPolicy, DbError> {
    Ok(resolve(config, &settings(conn)?).0)
}

/// Whether a file of this name and type may be stored, see
/// [`app_protocol::files::allows`].
pub fn allows(policy: &FilesPolicy, name: &str, mime: &str) -> bool {
    app_protocol::files::allows(policy, name, mime)
}

/// What the first bytes of a file say it is, for the common formats whose
/// name could be changed to slip past a rule: an extension and a type.
pub fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    const SIGNATURES: &[(&[u8], &str, &str)] = &[
        (
            b"MZ",
            ".exe",
            "application/vnd.microsoft.portable-executable",
        ),
        (b"\x7fELF", ".elf", "application/x-executable"),
        (b"%PDF-", ".pdf", "application/pdf"),
        (b"\x89PNG\r\n\x1a\n", ".png", "image/png"),
        (b"\xff\xd8\xff", ".jpg", "image/jpeg"),
        (b"GIF8", ".gif", "image/gif"),
        (b"PK\x03\x04", ".zip", "application/zip"),
    ];
    SIGNATURES
        .iter()
        .find(|(magic, _, _)| bytes.starts_with(magic))
        .map(|(_, ext, mime)| (*ext, *mime))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(allowed: &[&str], blocked: &[&str]) -> FilesPolicy {
        FilesPolicy {
            allowed_types: allowed.iter().map(|s| (*s).to_owned()).collect(),
            blocked_types: blocked.iter().map(|s| (*s).to_owned()).collect(),
            ..resolve(&FilesConfig::default(), &PolicySettings::default()).0
        }
    }

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn rules_match_extensions_and_types() {
        let open = policy(&[], &[".exe", "video/*"]);
        assert!(allows(&open, "a.PDF", "application/pdf"));
        assert!(!allows(&open, "setup.EXE", "application/octet-stream"));
        assert!(!allows(&open, "clip.mov", "video/quicktime"));
        let images = policy(&["image/*", ".pdf"], &["image/svg+xml"]);
        assert!(allows(&images, "x.png", "image/png"));
        assert!(allows(&images, "doc.pdf", "application/octet-stream"));
        assert!(!allows(&images, "x.svg", "image/svg+xml"));
        assert!(!allows(&images, "notes.txt", "text/plain"));
    }

    #[test]
    fn sniffs_what_a_name_could_hide() {
        assert_eq!(sniff(b"MZ\x90\x00").map(|s| s.0), Some(".exe"));
        assert_eq!(sniff(b"%PDF-1.7").map(|s| s.1), Some("application/pdf"));
        assert_eq!(sniff(b"plain text"), None);
    }

    #[test]
    fn the_instance_then_config() {
        let config = FilesConfig::default();
        let mut instance = PolicySettings::default();
        let (p, sources) = resolve(&config, &instance);
        assert_eq!(p.max_file_bytes, 250 * MB);
        assert_eq!(sources["maxFileBytes"].level, "config");
        instance.max_file_mb = Some(50);
        instance.quota_mb = Some(1000);
        let (p, sources) = resolve(&config, &instance);
        assert_eq!(p.max_file_bytes, 50 * MB);
        assert_eq!(p.quota_bytes, 1000 * MB);
        assert_eq!(sources["quotaBytes"].level, "instance");
        assert_eq!(sources["imageQuality"].level, "config");
    }

    #[test]
    fn bad_settings_are_named() {
        let bad = |s: PolicySettings| invalid(&s).is_some();
        assert!(bad(PolicySettings {
            image_quality: Some(0),
            ..PolicySettings::default()
        }));
        assert!(bad(PolicySettings {
            blocked_types: Some(list(&["exe"])),
            ..PolicySettings::default()
        }));
        assert!(bad(PolicySettings {
            allowed_types: Some(list(&[".p df"])),
            ..PolicySettings::default()
        }));
        assert!(!bad(PolicySettings {
            allowed_types: Some(list(&[".pdf", "image/*"])),
            image_quality: Some(70),
            ..PolicySettings::default()
        }));
    }
}
