// SPDX-License-Identifier: AGPL-3.0-or-later

//! Automatic backups: the vault's backup file (see [`crate::backup`]) made
//! in a folder on a schedule, the newest few kept. Their settings are kept
//! in the vault beside the old snapshot backups' (see [`crate::repo`]),
//! which are now only read, to restore from.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::clock::{Timestamp, utc};
use crate::repo::{BackupConfig, NO_PASSWORD, Schedule, TargetConfig};
use crate::store::BackupData;

/// How many automatic backups are kept when nothing else is said.
pub const DEFAULT_KEEP: u32 = 10;
/// The most that may be kept.
pub const MAX_KEEP: u32 = 100;

const fn default_keep() -> u32 {
    DEFAULT_KEEP
}

/// This device's automatic backups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoBackups {
    /// The folder chosen: a path, or a `content://` address on Android. The
    /// files go in a folder of their own inside it (see
    /// [`crate::repo::folder_name`]).
    pub folder: String,
    /// When.
    #[serde(default)]
    pub schedule: Schedule,
    /// How many automatic backups stay.
    #[serde(default = "default_keep")]
    pub keep: u32,
    /// The notes password the files are sealed with, none for notes
    /// without one.
    #[serde(default)]
    pub password: Option<String>,
}

/// Snapshot backups made in a folder: the files go to that folder now, on
/// the same schedule, with the same password. Others (S3, `WebDAV`) wait
/// for a folder to be chosen.
///
/// # Errors
///
/// Database errors.
pub fn from_snapshot_folder(conn: &rusqlite::Connection) -> crate::error::Result<()> {
    use rusqlite::OptionalExtension;
    let old: Option<String> = conn
        .query_row("SELECT config FROM backup_settings WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    // settings that do not read are left as they are, the page offers to
    // set backups up again
    let Some(old) = old.and_then(|text| serde_json::from_str::<BackupConfig>(&text).ok()) else {
        return Ok(());
    };
    let TargetConfig::Folder { path } = old.target else {
        return Ok(());
    };
    let files = AutoBackups {
        folder: path,
        schedule: old.schedule,
        keep: DEFAULT_KEEP,
        password: Some(old.password).filter(|p| p != NO_PASSWORD),
    };
    let text =
        serde_json::to_string(&files).map_err(|e| crate::error::Error::Invalid(e.to_string()))?;
    conn.execute("UPDATE backup_settings SET files = ?1 WHERE id = 1", [text])?;
    Ok(())
}

/// The name of the automatic backup made at `at`, in local time
/// (`offset_seconds` east of UTC): letters, digits and dashes only, which
/// every folder takes, and in the order they were made.
#[must_use]
pub fn file_name(at: Timestamp, offset_seconds: i64) -> String {
    let t = utc(at + offset_seconds * 1000);
    format!(
        "{}-{:04}-{:02}-{:02}-{:02}{:02}{:02}.{}",
        app_brand::SLUG,
        t.year,
        t.month,
        t.day,
        t.hour,
        t.minute,
        t.second,
        app_brand::BACKUP_EXTENSION
    )
}

/// Whether `name` is one [`file_name`] gave: only those are ever deleted,
/// a backup put in the folder by hand stays.
#[must_use]
pub fn is_automatic(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix(app_brand::SLUG)
        .and_then(|r| r.strip_prefix('-'))
        .and_then(|r| r.strip_suffix(app_brand::BACKUP_EXTENSION))
        .and_then(|r| r.strip_suffix('.'))
    else {
        return false;
    };
    // 2026-11-21-140005
    let b = rest.as_bytes();
    b.len() == 17
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 | 10 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// The automatic backups to delete so that the newest `keep` stay.
#[must_use]
pub fn beyond(names: &[String], keep: u32) -> Vec<String> {
    let mut automatic: Vec<&String> = names.iter().filter(|n| is_automatic(n)).collect();
    // the names sort as the times they hold
    automatic.sort_unstable_by(|a, b| b.cmp(a));
    automatic
        .into_iter()
        .skip(keep.max(1) as usize)
        .cloned()
        .collect()
}

/// What a backup would hold, as one hash: a run with the same one as the
/// last makes no new file.
///
/// # Errors
///
/// When the data does not serialize.
pub fn digest(data: &BackupData, files: &[String]) -> crate::error::Result<String> {
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(data).map_err(|e| crate::error::Error::Invalid(e.to_string()))?);
    for file in files {
        hash.update(file.as_bytes());
    }
    Ok(hash.finalize().iter().fold(String::new(), |mut hex, b| {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
        hex
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sort_as_their_times_and_are_told_apart() {
        let a = file_name(1_792_742_400_000, 0);
        let b = file_name(1_792_742_400_000 + 61_000, 0);
        assert_eq!(
            a,
            format!(
                "{}-2026-10-23-080000.{}",
                app_brand::SLUG,
                app_brand::BACKUP_EXTENSION
            )
        );
        assert!(a < b);
        assert!(is_automatic(&a));
        assert!(!is_automatic(&format!(
            "{} 2026-10-23.{}",
            app_brand::DISPLAY_NAME,
            app_brand::BACKUP_EXTENSION
        )));
        assert!(!is_automatic(&format!(
            "{}-2026-10-23-0800.{}",
            app_brand::SLUG,
            app_brand::BACKUP_EXTENSION
        )));
        assert!(!is_automatic("README.txt"));
    }

    #[test]
    fn local_time_names_the_file() {
        assert!(file_name(1_792_742_400_000, 3600).contains("-090000."));
    }

    #[test]
    fn only_automatic_backups_beyond_the_newest_go() {
        let at = |m: i64| file_name(1_792_742_400_000 + m * 60_000, 0);
        let mine = format!(
            "{} kept by hand.{}",
            app_brand::DISPLAY_NAME,
            app_brand::BACKUP_EXTENSION
        );
        let names = vec![at(1), at(3), mine.clone(), at(2), at(4)];
        assert_eq!(beyond(&names, 2), vec![at(2), at(1)]);
        assert_eq!(beyond(&names, 10), Vec::<String>::new());
        // at least one stays
        assert_eq!(beyond(&names, 0).len(), 3);
    }

    #[test]
    fn a_folder_of_snapshot_backups_becomes_the_files_folder() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("db/014_backups.sql"))
            .unwrap();
        conn.execute_batch(include_str!("db/023_file_backups.sql"))
            .unwrap();
        let old = |password: &str, kind: &str| {
            format!(
                r#"{{"target":{{"kind":"{kind}","path":"/home/me/Backups"}},"password":"{password}","schedule":"weekly","policy":{{"last":10,"daily":7,"weekly":4,"monthly":12,"yearly":0}},"host":"Desk"}}"#
            )
        };
        let files = |config: String| {
            conn.execute(
                "INSERT INTO backup_settings (id, config) VALUES (1, ?1)
                 ON CONFLICT (id) DO UPDATE SET config = excluded.config, files = NULL",
                [config],
            )
            .unwrap();
            from_snapshot_folder(&conn).unwrap();
            conn.query_row("SELECT files FROM backup_settings", [], |r| {
                r.get::<_, Option<String>>(0)
            })
            .unwrap()
            .map(|text| serde_json::from_str::<AutoBackups>(&text).unwrap())
        };
        assert_eq!(
            files(old("secret words", "folder")),
            Some(AutoBackups {
                folder: "/home/me/Backups".into(),
                schedule: Schedule::Weekly,
                keep: DEFAULT_KEEP,
                password: Some("secret words".into()),
            })
        );
        // made without a password: none now either
        assert_eq!(files(old(NO_PASSWORD, "folder")).unwrap().password, None);
    }
}
