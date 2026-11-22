// SPDX-License-Identifier: AGPL-3.0-or-later

//! Scheduled backups of this device: how often, and how the runs went.
//! Kept in the vault, with the settings of earlier versions' snapshot
//! backups.

use serde::{Deserialize, Serialize};

/// The password of snapshot backups made without one, on a device whose
/// notes are not encrypted.
pub const NO_PASSWORD: &str = "ficelle-backup-without-password";

/// The folder backups get inside the chosen one. A folder the app named
/// itself (Android's, "Ficelle Preview backups" in a preview build) is
/// already that folder.
#[must_use]
pub fn folder_name(chosen: &str) -> String {
    let own = std::path::Path::new(chosen)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| n.starts_with(app_brand::DISPLAY_NAME) && n.ends_with(" backups"));
    own.map_or_else(
        || format!("{} backups", app_brand::DISPLAY_NAME),
        str::to_owned,
    )
}

/// Where the snapshot backups of earlier versions went. Only a folder is
/// read now, the other places are left as they are.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum TargetConfig {
    /// A folder on this device, a drive, a share mounted as a folder.
    Folder {
        /// Its path.
        path: String,
    },
}

/// How often backups run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Schedule {
    /// Only when asked.
    Off,
    /// Every hour.
    Hourly,
    /// Every day.
    #[default]
    Daily,
    /// Every week.
    Weekly,
}

impl Schedule {
    /// The time between runs, in milliseconds. None when off.
    pub fn period_ms(self) -> Option<i64> {
        match self {
            Self::Off => None,
            Self::Hourly => Some(3_600_000),
            Self::Daily => Some(86_400_000),
            Self::Weekly => Some(7 * 86_400_000),
        }
    }
}

/// The snapshot backups' settings an earlier version kept.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct BackupConfig {
    /// Where.
    pub target: TargetConfig,
    /// The repository's password.
    pub password: String,
    /// When.
    pub schedule: Schedule,
}

/// How the runs went.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupStatus {
    /// The last run started, unix milliseconds.
    pub last_run_at: Option<i64>,
    /// The last run that went through.
    pub last_success_at: Option<i64>,
    /// Why the last run failed, None when it went through.
    pub last_error: Option<String>,
    /// Its kind (`notFound`, `wrongPassword`, `target`...), for the page
    /// to say it in the person's language.
    pub last_error_kind: Option<String>,
    /// Bytes at the target after the last run.
    pub stored_bytes: u64,
    /// The automatic backup files in the folder after the last run.
    pub files: u64,
    /// What the last automatic backup held, see [`crate::auto_backup::digest`].
    pub last_digest: Option<String>,
}
