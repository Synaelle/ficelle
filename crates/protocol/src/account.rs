// SPDX-License-Identifier: AGPL-3.0-or-later

//! Accounts, sign-in and devices. Times are unix milliseconds, ids are
//! uuid strings.

use serde::{Deserialize, Serialize};

use crate::e2ee::{AccountKind, AccountKinds};

/// Who may create an account on this server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Registration {
    /// Only admins add people, directly or with invite links. The default.
    Closed,
    /// Anyone with an account can invite.
    Invite,
    /// Anyone can register.
    Open,
}

/// What the sign-in screen needs before anyone signs in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthOptions {
    /// Registration mode.
    pub registration: Registration,
    /// No admin yet: the first one is made with the setup link from the log.
    pub setup_needed: bool,
    /// Passkeys can sign in.
    pub passkeys: bool,
    /// Which kinds of accounts sign-up creates.
    pub account_kinds: AccountKinds,
    /// A forgotten password can be reset by email.
    pub email_reset: bool,
    /// Label of the single sign-on button, absent without single sign-on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sso: Option<String>,
}

/// The device signing in, shown in the device list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Name the person recognises, for example "Pixel 7" or "Work laptop".
    pub name: String,
    /// `android`, `ios`, `windows`, `macos`, `linux` or `web`.
    pub platform: String,
    /// The id of this browser's or app's last sign-in, which this one takes
    /// the place of: the admin page forgets its session with the tab, and
    /// its old sign-in would stay listed. Signed out when it belongs to the
    /// same account. Servers before 0.2.1 ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
}

/// `POST /api/v1/auth/register`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    /// New username.
    pub username: String,
    /// New password.
    pub password: String,
    /// Name shown to others, the username when absent.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Invite token, needed unless registration is open.
    #[serde(default)]
    pub invite: Option<String>,
    /// This device.
    pub device: DeviceInfo,
}

/// `POST /api/v1/auth/login`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    /// Username, any letter case.
    pub username: String,
    /// Password.
    pub password: String,
    /// This device.
    pub device: DeviceInfo,
}

/// `POST /api/v1/auth/refresh`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    /// The refresh token from the last sign-in or refresh. It works once.
    pub refresh_token: String,
}

/// A signed in device: send `accessToken` as `Authorization: Bearer`, and
/// trade `refreshToken` for a new pair before the access token expires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// Short lived token for api calls.
    pub access_token: String,
    /// When the access token stops working.
    pub access_expires_at: i64,
    /// Single use token for the next pair.
    pub refresh_token: String,
    /// When the refresh token stops working.
    pub refresh_expires_at: i64,
    /// This device's id, to recognise it in the device list.
    pub device_id: String,
    /// The account.
    pub user: User,
}

/// Account role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// Runs the instance.
    Admin,
    /// Everyone else.
    User,
}

/// An account as its owner sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// Id.
    pub id: String,
    /// Username, lower case.
    pub username: String,
    /// Name shown to others.
    pub display_name: String,
    /// Role.
    pub role: Role,
    /// Standard or end-to-end encrypted.
    pub kind: AccountKind,
    /// When the account was created.
    pub created_at: i64,
    /// An admin who must add a passkey or an authenticator app before the
    /// admin pages answer it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub factor_required: bool,
}

/// `POST /api/v1/me/password`. Signs out every other device.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    /// Current password.
    pub current: String,
    /// New password.
    pub new: String,
}

/// One signed in device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// Id.
    pub id: String,
    /// Name given at sign-in.
    pub name: String,
    /// Platform given at sign-in.
    pub platform: String,
    /// First sign-in.
    pub created_at: i64,
    /// Last request, to the nearest few minutes.
    pub last_seen_at: i64,
    /// The device making this request.
    pub current: bool,
}

/// `POST /api/v1/invites`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteRequest {
    /// Days before it expires, the server default when absent.
    #[serde(default)]
    pub days: Option<u32>,
    /// Hours before it expires, for a short one, used before `days`. Up to
    /// a day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours: Option<u32>,
}

/// An invite. `token` and `link` appear only in the answer that creates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    /// Id, to revoke it.
    pub id: String,
    /// The secret part, to paste into the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Ready to send link, when the server knows its public address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Who made it.
    pub created_by: String,
    /// When.
    pub created_at: i64,
    /// Stops working then.
    pub expires_at: i64,
    /// `pending`, `used`, `revoked` or `expired`.
    pub state: String,
}

/// `POST /api/v1/admin/users`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    /// Username.
    pub username: String,
    /// First password, to pass on to the person.
    pub password: String,
    /// Name shown to others, the username when absent.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Role, user when absent.
    #[serde(default)]
    pub role: Option<Role>,
}

/// `PUT /api/v1/admin/registration`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationRequest {
    /// New mode.
    pub registration: Registration,
}
