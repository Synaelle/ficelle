// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin api: people, groups, the security log and the instance at a
//! glance. Times are unix milliseconds.

use crate::e2ee::{AccountKind, AccountKinds};
use serde::{Deserialize, Serialize};

use crate::account::{Device, Registration, Role};

/// `GET /api/v1/admin/overview`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// Server software version.
    pub version: String,
    /// When this instance was first started.
    pub created_at: String,
    /// Accounts.
    pub users: u64,
    /// Of which admins.
    pub admins: u64,
    /// Of which turned off.
    pub disabled: u64,
    /// Groups.
    pub groups: u64,
    /// Devices signed in now.
    pub devices: u64,
    /// Registration mode.
    pub registration: Registration,
    /// Which kinds of accounts sign-up creates.
    pub account_kinds: AccountKinds,
    /// End-to-end encrypted accounts.
    pub e2ee: u64,
    /// Open notes follow other devices' edits live.
    pub coediting: bool,
    /// Admins act as admin only once they have a second step.
    #[serde(default = "yes")]
    pub admins_need_second_step: bool,
    /// The server is for its admin alone, chosen at setup: the admin page
    /// shows only what is about one person.
    #[serde(default)]
    pub personal: bool,
}

/// `PUT /api/v1/admin/personal`: the server for its admin alone, or for
/// others too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalRequest {
    /// For the admin alone.
    pub personal: bool,
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

/// An account as an admin sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminUser {
    /// Id.
    pub id: String,
    /// Username.
    pub username: String,
    /// Shown name.
    pub display_name: String,
    /// Role.
    pub role: Role,
    /// Creation time.
    pub created_at: i64,
    /// Set when turned off.
    pub disabled_at: Option<i64>,
    /// Set while sign-in is locked after failures.
    pub locked_until: Option<i64>,
    /// Latest request from any device.
    pub last_seen_at: Option<i64>,
    /// Devices signed in.
    pub devices: u64,
    /// Groups, by name.
    pub groups: Vec<String>,
    /// Signs in with a second factor.
    pub second_factor: bool,
    /// Standard or end-to-end encrypted.
    pub kind: AccountKind,
    /// The confirmed email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Set while the account waits for its deletion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_at: Option<i64>,
}

/// One account with its devices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserDetail {
    /// The account.
    pub user: AdminUser,
    /// Its signed in devices. `current` is always false here.
    pub device_list: Vec<Device>,
}

/// A page of a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    /// Matches in all.
    pub total: u64,
    /// This page.
    pub items: Vec<T>,
}

/// `PATCH /api/v1/admin/users/{id}`: only the fields present change.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    /// New shown name.
    #[serde(default)]
    pub display_name: Option<String>,
    /// New role.
    #[serde(default)]
    pub role: Option<Role>,
    /// Turn the account off (signs out its devices) or back on.
    #[serde(default)]
    pub disabled: Option<bool>,
    /// Lift a lock after failed sign-ins.
    #[serde(default)]
    pub unlock: bool,
}

/// `POST /api/v1/admin/users/{id}/password`: signs out every device.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPasswordRequest {
    /// New password, to pass on to the person.
    pub password: String,
}

/// A group of people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    /// Id.
    pub id: String,
    /// Name, unique whatever the letter case.
    pub name: String,
    /// Creation time.
    pub created_at: i64,
    /// Members.
    pub members: u64,
}

/// A group with its members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDetail {
    /// The group.
    pub group: Group,
    /// Members, by username.
    pub member_list: Vec<GroupMember>,
}

/// One member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMember {
    /// Account id.
    pub user_id: String,
    /// Username.
    pub username: String,
    /// Shown name.
    pub display_name: String,
    /// When added.
    pub added_at: i64,
}

/// `POST /api/v1/admin/groups` and `PATCH .../{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRequest {
    /// Name.
    pub name: String,
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

/// A files policy as set at one level, the instance, a group or a person.
/// A field left out takes the level below: a person's group, then the
/// instance, then the server's config file.
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
    /// `user`, `group`, `instance` or `config`.
    pub level: String,
    /// The groups whose value it is, for `group`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
}

/// `GET /api/v1/admin/policy`, `.../groups/{id}/policy`,
/// `.../users/{id}/policy`: what is set at that level and what follows.
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

/// What one person stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonStorage {
    /// Account id.
    pub user_id: String,
    /// Username.
    pub username: String,
    /// Shown name.
    pub display_name: String,
    /// Counted against the quota: files and notes of the spaces they own.
    pub used_bytes: u64,
    /// Of which files.
    pub file_bytes: u64,
    /// Number of files.
    pub files: u64,
    /// The largest one.
    pub largest_bytes: u64,
    /// Their quota, 0 for no limit.
    pub quota_bytes: u64,
}

/// `GET /api/v1/admin/storage?offset=&limit=`: the instance's storage and
/// people by what they use, most first.
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
    /// People, most used first.
    pub people: Page<PersonStorage>,
}

/// `POST /api/v1/admin/mail/test`: a test mail, sent at once so the answer
/// says whether the mail server took it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailTestRequest {
    /// Where it goes.
    pub to: String,
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
