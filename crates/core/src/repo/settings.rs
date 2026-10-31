// SPDX-License-Identifier: AGPL-3.0-or-later

//! Scheduled backups of this device: where, with which password, how
//! often, what to keep, and how the runs went. Kept in the vault.

use app_repo::{FolderTarget, Policy, S3Config, S3Target, Target, WebdavConfig, WebdavTarget};
use serde::{Deserialize, Serialize};

use super::RunReport;
use crate::error::Result;

/// Where backups go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TargetConfig {
    /// A folder on this device, a drive, a share mounted as a folder.
    Folder {
        /// Its path.
        path: String,
    },
    /// An S3 bucket.
    S3(S3Config),
    /// A `WebDAV` folder.
    Webdav(WebdavConfig),
}

impl TargetConfig {
    /// The target to use.
    ///
    /// # Errors
    ///
    /// An incomplete config.
    pub fn open(&self) -> Result<std::sync::Arc<dyn Target>> {
        Ok(match self {
            Self::Folder { path } => {
                if path.trim().is_empty() {
                    return Err(crate::error::Error::Invalid("choose a folder".into()));
                }
                std::sync::Arc::new(FolderTarget::new(path))
            }
            Self::S3(config) => std::sync::Arc::new(S3Target::new(config.clone())?),
            Self::Webdav(config) => std::sync::Arc::new(WebdavTarget::new(config)?),
        })
    }

    /// The same without its secrets, to show.
    #[must_use]
    pub fn without_secrets(&self) -> Self {
        match self {
            Self::Folder { path } => Self::Folder { path: path.clone() },
            Self::S3(c) => Self::S3(S3Config {
                secret_key: String::new(),
                ..c.clone()
            }),
            Self::Webdav(c) => Self::Webdav(WebdavConfig {
                password: String::new(),
                ..c.clone()
            }),
        }
    }

    /// The same with the secrets of `old` where these are empty: a form
    /// shows them blank and sends them back blank when unchanged.
    #[must_use]
    pub fn keeping_secrets_of(self, old: &Self) -> Self {
        match (self, old) {
            (Self::S3(mut new), Self::S3(old)) if new.secret_key.is_empty() => {
                new.secret_key.clone_from(&old.secret_key);
                Self::S3(new)
            }
            (Self::Webdav(mut new), Self::Webdav(old)) if new.password.is_empty() => {
                new.password.clone_from(&old.password);
                Self::Webdav(new)
            }
            (new, _) => new,
        }
    }
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

/// Everything about this device's backups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupConfig {
    /// Where.
    pub target: TargetConfig,
    /// The repository's password.
    pub password: String,
    /// When.
    pub schedule: Schedule,
    /// What to keep.
    pub policy: Policy,
    /// This device's name in the repository's snapshots.
    pub host: String,
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
    /// What the last good run did.
    pub last_report: Option<RunReport>,
    /// Snapshots in the repository after the last run.
    pub snapshots: u64,
    /// Bytes at the target after the last run.
    pub stored_bytes: u64,
}
