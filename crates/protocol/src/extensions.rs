// SPDX-License-Identifier: AGPL-3.0-or-later

//! The instance's rule for themes, and the ones its admin offers. The admin
//! sets them (`/admin/extensions`), the apps signed in read them
//! (`/sync/extensions`) and keep them. Times are unix milliseconds.

use serde::{Deserialize, Serialize};

/// Which themes the apps may use, besides the built-in ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeRule {
    /// Any theme the person imports.
    #[default]
    Any,
    /// Only the ones this instance offers.
    Instance,
}

/// The rule, the same for everyone on the instance.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExtensionRules {
    /// Which themes may be used.
    pub themes: ThemeRule,
}

/// A theme the admin uploaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferedTheme {
    /// The theme's id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Of the theme file.
    pub size: u64,
    /// Hex SHA-256 of the theme file.
    pub sha256: String,
    /// Added on every device, not only listed.
    pub preinstall: bool,
    /// When the admin uploaded this version.
    pub uploaded_at: i64,
}

/// `GET /api/v1/admin/extensions` and `GET /api/v1/sync/extensions`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Extensions {
    /// The rule.
    pub rules: ExtensionRules,
    /// The themes offered, by name.
    pub themes: Vec<OfferedTheme>,
}

/// `PATCH /api/v1/admin/extensions/themes/{id}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPreinstall {
    /// Offered on every device, or only listed.
    pub preinstall: bool,
}
