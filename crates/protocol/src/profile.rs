// SPDX-License-Identifier: AGPL-3.0-or-later

//! What people do with their own account besides signing in: an email for
//! resets, a password reset, an export of their data, deleting the
//! account. And what the privacy page shows.

use serde::{Deserialize, Serialize};

use crate::e2ee::AccountKinds;

/// `PUT /api/v1/me/email`: sends a link to the address, which becomes the
/// account's once opened. Needs the password, see `Confirm`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailRequest {
    /// The address.
    pub email: String,
    /// Current password.
    #[serde(default)]
    pub password: Option<String>,
    /// For an e2ee account, the proof from `/me/confirm`.
    #[serde(default)]
    pub proof: Option<String>,
}

/// `POST /api/v1/auth/email/verify`: the token from the link.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailVerifyRequest {
    /// From the link.
    pub token: String,
}

/// Answer of [`EmailVerifyRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailVerified {
    /// The account's username.
    pub username: String,
    /// The address now kept.
    pub email: String,
}

/// `POST /api/v1/auth/reset/request`. Always answers 202, whether or not
/// the account exists or has an address, so it tells nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetRequest {
    /// Username, any letter case.
    pub username: String,
}

/// `POST /api/v1/auth/reset/finish`: the new password, with the token from
/// the mail. Signs out every device.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetFinish {
    /// From the link.
    pub token: String,
    /// The new password.
    pub password: String,
}

/// `POST /api/v1/me/deletion`: deletes the account after the grace period.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionRequest {
    /// Current password.
    #[serde(default)]
    pub password: Option<String>,
    /// For an e2ee account, the proof from `/me/confirm`.
    #[serde(default)]
    pub proof: Option<String>,
}

/// A pending deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deletion {
    /// When the account goes. Signing in and cancelling before keeps it.
    pub delete_at: i64,
}

/// `GET /api/v1/privacy`: facts for the privacy page, from the server's
/// configuration. Needs no sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyInfo {
    /// Who runs the server, when the admin said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<String>,
    /// How to reach them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    /// More from them, plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// Which kinds of accounts sign-up creates.
    pub account_kinds: AccountKinds,
    /// The server sends email.
    pub email: bool,
    /// Single sign-on provider's label, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sso: Option<String>,
    /// Days the security log keeps an entry.
    pub audit_days: u32,
    /// Days between asking for deletion and the deletion.
    pub deletion_grace_days: u32,
    /// Days a device stays signed in without being used.
    pub session_days: u32,
    /// This server takes feedback reports from anyone's apps.
    #[serde(default)]
    pub feedback_receive: bool,
    /// Days it keeps a report.
    #[serde(default)]
    pub feedback_keep_days: u32,
    /// Where its people's reports go.
    #[serde(default)]
    pub feedback_send_to: crate::feedback::SendTo,
    /// The project's main instance, named when reports go there.
    #[serde(default)]
    pub feedback_main: String,
}
