// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin api: the security log, the files policy and the instance at
//! a glance. Times are unix milliseconds.

use serde::{Deserialize, Serialize};

/// `GET /api/v1/admin/overview`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// Server software version.
    pub version: String,
    /// When this instance was first started.
    pub created_at: String,
    /// Devices signed in now.
    pub devices: u64,
    /// Open notes follow other devices' edits live.
    pub coediting: bool,
    /// Admins act as admin only once they have a second step.
    #[serde(default = "yes")]
    pub admins_need_second_step: bool,
}

fn yes() -> bool {
    true
}

/// `PUT /api/v1/admin/second-step`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecondStepRequest {
    /// Admins must have a second step.
    pub required: bool,
}

/// `PUT /api/v1/admin/coediting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoeditingRequest {
    /// On or off for every account.
    pub coediting: bool,
}

/// One line of the security log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    /// Id, increasing.
    pub id: i64,
    /// When.
    pub at: i64,
    /// What, for example `sign_in`.
    pub event: String,
    /// Account concerned.
    pub user_id: Option<String>,
    /// Its username, when the account still exists.
    pub username: Option<String>,
    /// Device concerned.
    pub device_id: Option<String>,
    /// Client address.
    pub ip: Option<String>,
    /// More.
    pub detail: Option<String>,
}

/// `GET /api/v1/admin/audit`: newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditPage {
    /// Entries.
    pub items: Vec<AuditEntry>,
    /// Pass as `before` for the next older page, absent at the end.
    pub next_before: Option<i64>,
}

/// The instance's files policy as set in the admin page. A field left out
/// takes the server's config file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicySettings {
    /// Largest file in megabytes, 0 for no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_file_mb: Option<u32>,
    /// Storage in megabytes, 0 for no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_mb: Option<u32>,
    /// Types allowed, empty for every type not blocked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_types: Option<Vec<String>>,
    /// Types refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_types: Option<Vec<String>>,
    /// Attachments per note, 0 for no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_files_per_note: Option<u32>,
    /// Longest side images are shrunk to, 0 to keep them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_max_px: Option<u32>,
    /// Quality of shrunk images, 1 to 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_quality: Option<u32>,
    /// Small previews of images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnails: Option<bool>,
    /// Days a file no longer used is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_days: Option<u32>,
}

/// Where a value in effect comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicySource {
    /// `instance` or `config`.
    pub level: String,
}

/// `GET /api/v1/admin/policy`: what is set and what follows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyView {
    /// Set at this level.
    pub settings: PolicySettings,
    /// In effect there, for a person what applies to them.
    pub effective: crate::files::FilesPolicy,
    /// For each key of `effective`, by its camel case name, where it comes
    /// from.
    pub sources: std::collections::BTreeMap<String, PolicySource>,
}

/// `GET /api/v1/admin/storage`: the instance's storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageOverview {
    /// Bytes the file backend holds, encryption included.
    pub stored_bytes: u64,
    /// Stored files.
    pub files: u64,
    /// What spaces use, counting a file once per space: the difference
    /// with the files' own size is what keeping one copy saves.
    pub referenced_bytes: u64,
    /// The files' own size, each once.
    pub unique_bytes: u64,
    /// Synced notes, all spaces.
    pub note_bytes: u64,
}

/// `GET /api/v1/admin/checklist`: the security checklist on the dashboard.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checklist {
    /// In the order shown.
    pub items: Vec<ChecklistItem>,
}

/// One line of the checklist. The page words it from `key` and `state`,
/// with `values` filled in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistItem {
    /// `secondFactors`, `registration`, `backups`, `updates`, `tls`,
    /// `address` or `secrets`.
    pub key: String,
    /// How it stands.
    pub state: CheckState,
    /// What the line names: people, dates, versions.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub values: std::collections::BTreeMap<String, String>,
    /// An admin said it is fine as it is: the page hides it. It shows again
    /// once the line says something else.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dismissed: bool,
}

/// How a checklist line stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckState {
    /// Nothing to do.
    Ok,
    /// Worth a look.
    Warn,
    /// Fix it.
    Fail,
}
