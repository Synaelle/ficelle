// SPDX-License-Identifier: AGPL-3.0-or-later

//! End-to-end encrypted accounts. Their password never reaches the server:
//! sign-in and sign-up run OPAQUE, every OPAQUE message travels as
//! base64url text. The server keeps the account keys wrapped, it cannot
//! open them.

use serde::{Deserialize, Serialize};

use crate::account::DeviceInfo;

/// How the password of an OPAQUE record is stretched, as `app_e2ee::Ksf`
/// numbers it: 1 for records made before 0.3.0, 2 since.
pub const KSF_CURRENT: u8 = 2;

/// What a message without the field means: a client or server from before
/// stretching had a number.
fn ksf_before() -> u8 {
    1
}

/// What kind an account is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountKind {
    /// The server stores notes encrypted at rest with its own key, it can
    /// search them and an admin can set a new password.
    Standard,
    /// Notes are encrypted on the devices. The server never sees the
    /// password or the content, nobody can reset the password: only the
    /// recovery code opens the account when it is forgotten.
    E2ee,
}

/// Which kinds new accounts can be, chosen by an admin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountKinds {
    /// Standard accounts only.
    Standard,
    /// End-to-end encrypted accounts only.
    E2ee,
    /// The person picks at sign-up. The default.
    Choice,
}

/// The master key wrapped twice, opaque to the server. See `app-e2ee`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyBundle {
    /// Format of the bundle.
    pub version: u32,
    /// Opened with the password.
    pub by_password: String,
    /// Opened with the recovery code.
    pub by_recovery: String,
}

/// `POST /api/v1/auth/kind`: how to sign in as `username`. An unknown
/// username gets a kind too, the same every time, so the answer does not
/// tell which accounts exist.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindRequest {
    /// Username, any letter case.
    pub username: String,
}

/// Answer of [`KindRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindAnswer {
    /// The kind to sign in as.
    pub kind: AccountKind,
}

/// `POST /api/v1/auth/opaque/registration`: the server's half of setting a
/// password, for a new account, a new password or a recovery. It keeps
/// nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueRegistrationRequest {
    /// Username the password is for.
    pub username: String,
    /// OPAQUE registration request.
    pub request: String,
}

/// Answer of [`OpaqueRegistrationRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueRegistrationResponse {
    /// OPAQUE registration response.
    pub response: String,
}

/// `POST /api/v1/auth/e2ee/register`: creates an e2ee account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eeRegisterRequest {
    /// Username, the one the registration was made for.
    pub username: String,
    /// OPAQUE registration upload.
    pub upload: String,
    /// The account keys, wrapped.
    pub keys: KeyBundle,
    /// Proof of the recovery code, the server keeps its hash.
    pub recovery_verifier: String,
    /// Name shown to others, the username when absent.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Invite token, needed unless registration is open.
    #[serde(default)]
    pub invite: Option<String>,
    /// This device.
    pub device: DeviceInfo,
    /// How the upload's password was stretched.
    #[serde(default = "ksf_before")]
    pub ksf: u8,
}

/// `POST /api/v1/auth/opaque/login/start`, and `/api/v1/me/confirm/start`
/// (where `username` is ignored).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueLoginStart {
    /// Username, any letter case.
    #[serde(default)]
    pub username: String,
    /// OPAQUE credential request.
    pub request: String,
}

/// A sign-in in progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueLoginStarted {
    /// Single use, send it back with the finalization.
    pub challenge: String,
    /// OPAQUE credential response.
    pub response: String,
    /// How the account's record stretches the password, the client does
    /// the same. An unknown account gets a number too.
    #[serde(default = "ksf_before")]
    pub ksf: u8,
}

/// `POST /api/v1/auth/opaque/login/finish`. Answers a session, or 202 and
/// a second factor challenge like a password sign-in.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueLoginFinish {
    /// From [`OpaqueLoginStarted`].
    pub challenge: String,
    /// OPAQUE credential finalization.
    pub finalization: String,
    /// This device.
    pub device: DeviceInfo,
}

/// `POST /api/v1/me/confirm/finish`: proves the password of a signed in
/// e2ee account, for changes to how it signs in.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmFinish {
    /// From [`OpaqueLoginStarted`].
    pub challenge: String,
    /// OPAQUE credential finalization.
    pub finalization: String,
}

/// Answer of [`ConfirmFinish`]: send it as `proof` with the change. It
/// works once, for five minutes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proof {
    /// The proof.
    pub proof: String,
}

/// `GET /api/v1/me/keys`: the wrapped account keys of an e2ee account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountKeys {
    /// The keys.
    pub keys: KeyBundle,
}

/// `POST /api/v1/me/e2ee/password`: a new password for a signed in e2ee
/// account. Signs out every other device. Also `/api/v1/me/e2ee/strengthen`:
/// the same password registered again with stronger stretching, which
/// signs nobody out.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eePasswordRequest {
    /// OPAQUE registration upload for the new password.
    pub upload: String,
    /// The keys with the password wrapping redone. The recovery wrapping
    /// stays as it was.
    pub keys: KeyBundle,
    /// Proof of the current password, from `/me/confirm`.
    pub proof: String,
    /// How the upload's password was stretched.
    #[serde(default = "ksf_before")]
    pub ksf: u8,
}

/// `POST /api/v1/auth/recovery/start`: a forgotten password, with the
/// recovery code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStart {
    /// Username, any letter case.
    pub username: String,
    /// Proof of the recovery code.
    pub verifier: String,
}

/// Answer of [`RecoveryStart`]: the keys, for the recovery code to open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStarted {
    /// Single use, five minutes.
    pub challenge: String,
    /// The account keys.
    pub keys: KeyBundle,
}

/// `POST /api/v1/auth/recovery/finish`: the new password. Signs out every
/// device, then signs this one in.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryFinish {
    /// From [`RecoveryStarted`].
    pub challenge: String,
    /// OPAQUE registration upload for the new password.
    pub upload: String,
    /// The keys with the password wrapping redone.
    pub keys: KeyBundle,
    /// This device.
    pub device: DeviceInfo,
    /// How the upload's password was stretched.
    #[serde(default = "ksf_before")]
    pub ksf: u8,
}

/// `PUT /api/v1/admin/account-kinds`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountKindsRequest {
    /// New policy.
    pub account_kinds: AccountKinds,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_on_the_wire() {
        assert_eq!(serde_json::to_value(AccountKind::E2ee).unwrap(), "e2ee");
        assert_eq!(
            serde_json::to_value(AccountKinds::Choice).unwrap(),
            "choice"
        );
        let bundle: KeyBundle =
            serde_json::from_str(r#"{"version":1,"byPassword":"a","byRecovery":"b"}"#).unwrap();
        assert_eq!(bundle.by_recovery, "b");
    }
}
