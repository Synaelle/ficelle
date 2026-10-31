// SPDX-License-Identifier: AGPL-3.0-or-later

//! The rules for an account's files, and the type checks.
//!
//! Each key is looked up on its own, the first level that sets it wins:
//!
//! 1. the person's own settings,
//! 2. the settings of their groups, combined (below),
//! 3. the instance settings from the admin page,
//! 4. `[files]` in the config file, whose defaults need nothing set.
//!
//! Someone in several groups that set a key gets the most generous value
//! among them (decided with the user): no limit beats any limit, otherwise
//! the largest size, quota, count, retention and image quality. Keeping
//! images as they are beats shrinking them, previews on beat off. A type is
//! allowed when any of those groups allows it (an empty list, every type,
//! beats any list, else the lists are joined) and blocked only when all of
//! them block it.

use std::collections::BTreeMap;

use app_protocol::admin::{PolicySettings, PolicySource};
use app_protocol::files::FilesPolicy;
use rusqlite::{Connection, OptionalExtension, params};

use crate::config::FilesConfig;
use crate::db::DbError;

const MB: u64 = 1024 * 1024;

/// Where settings are kept: the instance, a group or a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The whole server.
    Instance,
    /// A group, by id.
    Group,
    /// A person, by account id.
    User,
}

impl Scope {
    fn text(self) -> &'static str {
        match self {
            Self::Instance => "instance",
            Self::Group => "group",
            Self::User => "user",
        }
    }
}

/// The settings stored for a level, nothing set when there are none.
///
/// # Errors
///
/// Database errors, stored json that does not read.
pub fn settings(conn: &Connection, scope: Scope, target: &str) -> Result<PolicySettings, DbError> {
    let text: Option<String> = conn
        .query_row(
            "SELECT settings FROM policies WHERE scope = ?1 AND target = ?2",
            params![scope.text(), target],
            |row| row.get(0),
        )
        .optional()?;
    match text {
        Some(text) => serde_json::from_str(&text).map_err(|e| DbError::Data(e.to_string())),
        None => Ok(PolicySettings::default()),
    }
}

/// Stores a level's settings, or forgets them when nothing is set.
///
/// # Errors
///
/// Database errors.
pub fn set(
    conn: &Connection,
    scope: Scope,
    target: &str,
    settings: &PolicySettings,
    now: i64,
) -> Result<(), DbError> {
    if *settings == PolicySettings::default() {
        conn.execute(
            "DELETE FROM policies WHERE scope = ?1 AND target = ?2",
            params![scope.text(), target],
        )?;
        return Ok(());
    }
    let text = serde_json::to_string(settings).map_err(|e| DbError::Data(e.to_string()))?;
    conn.execute(
        "INSERT INTO policies (scope, target, settings, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (scope, target) DO UPDATE SET settings = excluded.settings,
             updated_at = excluded.updated_at",
        params![scope.text(), target, text, now],
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

/// The settings that apply to someone, level by level.
#[derive(Debug, Clone, Default)]
pub struct Levels {
    /// Their own.
    pub user: Option<PolicySettings>,
    /// Their groups', by group name.
    pub groups: Vec<(String, PolicySettings)>,
    /// The instance's.
    pub instance: PolicySettings,
}

/// The levels of an account.
///
/// # Errors
///
/// Database errors.
pub fn levels_of(conn: &Connection, user: &str) -> Result<Levels, DbError> {
    let groups: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT g.id, g.name FROM group_members m JOIN groups g ON g.id = m.group_id
             WHERE m.user_id = ?1 ORDER BY g.name",
        )?;
        stmt.query_map([user], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut set = Vec::new();
    for (id, name) in groups {
        set.push((name, settings(conn, Scope::Group, &id)?));
    }
    Ok(Levels {
        user: Some(settings(conn, Scope::User, user)?),
        groups: set,
        instance: settings(conn, Scope::Instance, "")?,
    })
}

/// The policy for someone with no rule of their own and no group: what the
/// whole server allows by default, for the dashboard's checklist.
///
/// # Errors
///
/// Database errors.
pub fn everyone(conn: &Connection, config: &FilesConfig) -> Result<FilesPolicy, DbError> {
    let levels = Levels {
        user: None,
        groups: Vec::new(),
        instance: settings(conn, Scope::Instance, "")?,
    };
    Ok(resolve(config, &levels).0)
}

/// No limit (0) beats any limit, else the larger.
fn limit(values: Vec<u32>) -> u32 {
    if values.contains(&0) {
        0
    } else {
        values.into_iter().max().unwrap_or(0)
    }
}

fn largest(values: Vec<u32>) -> u32 {
    values.into_iter().max().unwrap_or(0)
}

/// Every type any list allows, or all of them when one list is empty.
fn allowed(lists: Vec<Vec<String>>) -> Vec<String> {
    if lists.iter().any(Vec::is_empty) {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for rule in lists.into_iter().flatten() {
        if !out.iter().any(|r| r.eq_ignore_ascii_case(&rule)) {
            out.push(rule);
        }
    }
    out
}

/// The types every list blocks.
fn blocked(lists: Vec<Vec<String>>) -> Vec<String> {
    let mut iter = lists.into_iter();
    let first = iter.next().unwrap_or_default();
    let rest: Vec<Vec<String>> = iter.collect();
    first
        .into_iter()
        .filter(|rule| {
            rest.iter()
                .all(|list| list.iter().any(|r| r.eq_ignore_ascii_case(rule)))
        })
        .collect()
}

/// One key through the levels.
fn layer<T: Clone>(
    levels: &Levels,
    get: impl Fn(&PolicySettings) -> Option<T>,
    combine: impl Fn(Vec<T>) -> T,
    config: T,
) -> (T, PolicySource) {
    let source = |level: &str, groups: Vec<String>| PolicySource {
        level: level.into(),
        groups,
    };
    if let Some(value) = levels.user.as_ref().and_then(&get) {
        return (value, source("user", Vec::new()));
    }
    let from_groups: Vec<(String, T)> = levels
        .groups
        .iter()
        .filter_map(|(name, s)| get(s).map(|v| (name.clone(), v)))
        .collect();
    if !from_groups.is_empty() {
        let names = from_groups.iter().map(|(n, _)| n.clone()).collect();
        let value = combine(from_groups.into_iter().map(|(_, v)| v).collect());
        return (value, source("group", names));
    }
    match get(&levels.instance) {
        Some(value) => (value, source("instance", Vec::new())),
        None => (config, source("config", Vec::new())),
    }
}

/// Notes where a key's value comes from and hands the value on.
fn take<T>(sources: &mut BTreeMap<String, PolicySource>, key: &str, found: (T, PolicySource)) -> T {
    sources.insert(key.to_owned(), found.1);
    found.0
}

/// The policy in effect through the levels, and where each key comes from.
pub fn resolve(
    config: &FilesConfig,
    levels: &Levels,
) -> (FilesPolicy, BTreeMap<String, PolicySource>) {
    let mut sources = BTreeMap::new();
    let max_file_mb = take(
        &mut sources,
        "maxFileBytes",
        layer(levels, |s| s.max_file_mb, limit, config.max_file_mb),
    );
    let quota_mb = take(
        &mut sources,
        "quotaBytes",
        layer(levels, |s| s.quota_mb, limit, config.quota_mb),
    );
    let allowed_types = take(
        &mut sources,
        "allowedTypes",
        layer(
            levels,
            |s| s.allowed_types.clone(),
            allowed,
            config.allowed_types.clone(),
        ),
    );
    let blocked_types = take(
        &mut sources,
        "blockedTypes",
        layer(
            levels,
            |s| s.blocked_types.clone(),
            blocked,
            config.blocked_types.clone(),
        ),
    );
    let max_files_per_note = take(
        &mut sources,
        "maxFilesPerNote",
        layer(
            levels,
            |s| s.max_files_per_note,
            limit,
            config.max_files_per_note,
        ),
    );
    let image_max_px = take(
        &mut sources,
        "imageMaxPx",
        layer(levels, |s| s.image_max_px, limit, config.image_max_px),
    );
    let image_quality = take(
        &mut sources,
        "imageQuality",
        layer(levels, |s| s.image_quality, largest, config.image_quality),
    );
    let thumbnails = take(
        &mut sources,
        "thumbnails",
        layer(
            levels,
            |s| s.thumbnails,
            |v: Vec<bool>| v.contains(&true),
            config.thumbnails,
        ),
    );
    let retention_days = take(
        &mut sources,
        "retentionDays",
        layer(levels, |s| s.retention_days, largest, config.retention_days),
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

/// The policy in effect for an account.
///
/// # Errors
///
/// Database errors.
pub fn effective(
    conn: &Connection,
    config: &FilesConfig,
    user: &str,
) -> Result<FilesPolicy, DbError> {
    Ok(resolve(config, &levels_of(conn, user)?).0)
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
            ..resolve(&FilesConfig::default(), &Levels::default()).0
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
    fn the_person_then_groups_then_instance_then_config() {
        let config = FilesConfig::default();
        let mut levels = Levels::default();
        let (p, sources) = resolve(&config, &levels);
        assert_eq!(p.max_file_bytes, 100 * MB);
        assert_eq!(sources["maxFileBytes"].level, "config");
        levels.instance.max_file_mb = Some(50);
        levels.instance.quota_mb = Some(1000);
        let (p, sources) = resolve(&config, &levels);
        assert_eq!(p.max_file_bytes, 50 * MB);
        assert_eq!(sources["quotaBytes"].level, "instance");
        levels.groups = vec![(
            "Family".into(),
            PolicySettings {
                max_file_mb: Some(200),
                ..PolicySettings::default()
            },
        )];
        let (p, sources) = resolve(&config, &levels);
        assert_eq!(p.max_file_bytes, 200 * MB);
        assert_eq!(sources["maxFileBytes"].groups, vec!["Family"]);
        assert_eq!(p.quota_bytes, 1000 * MB, "a key the group leaves alone");
        // a person's own value wins, even a stricter one
        levels.user = Some(PolicySettings {
            max_file_mb: Some(10),
            ..PolicySettings::default()
        });
        let (p, sources) = resolve(&config, &levels);
        assert_eq!(p.max_file_bytes, 10 * MB);
        assert_eq!(sources["maxFileBytes"].level, "user");
    }

    #[test]
    fn several_groups_give_the_most_generous() {
        let config = FilesConfig::default();
        let levels = Levels {
            user: None,
            groups: vec![
                (
                    "A".into(),
                    PolicySettings {
                        max_file_mb: Some(20),
                        quota_mb: Some(0),
                        allowed_types: Some(list(&["image/*"])),
                        blocked_types: Some(list(&[".exe", "video/*"])),
                        image_max_px: Some(1600),
                        thumbnails: Some(false),
                        retention_days: Some(7),
                        ..PolicySettings::default()
                    },
                ),
                (
                    "B".into(),
                    PolicySettings {
                        max_file_mb: Some(80),
                        quota_mb: Some(500),
                        allowed_types: Some(list(&[".pdf"])),
                        blocked_types: Some(list(&[".EXE"])),
                        image_max_px: Some(0),
                        thumbnails: Some(true),
                        retention_days: Some(90),
                        ..PolicySettings::default()
                    },
                ),
            ],
            instance: PolicySettings::default(),
        };
        let (p, sources) = resolve(&config, &levels);
        assert_eq!(p.max_file_bytes, 80 * MB);
        assert_eq!(p.quota_bytes, 0, "no limit beats a limit");
        assert_eq!(p.allowed_types, vec!["image/*", ".pdf"]);
        assert_eq!(
            p.blocked_types,
            vec![".exe"],
            "blocked only where all block"
        );
        assert_eq!(p.image_max_px, 0, "keeping images wins");
        assert!(p.thumbnails);
        assert_eq!(p.retention_days, 90);
        assert_eq!(sources["quotaBytes"].groups, vec!["A", "B"]);
        // one group allowing every type opens them all
        let mut open = levels.clone();
        open.groups[1].1.allowed_types = Some(list(&[]));
        assert!(resolve(&config, &open).0.allowed_types.is_empty());
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
