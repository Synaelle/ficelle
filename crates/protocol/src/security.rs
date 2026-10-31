// SPDX-License-Identifier: AGPL-3.0-or-later

//! Second factors, passkeys and single sign-on. Passkey options and answers
//! are the WebAuthn json the browser api takes and returns, passed through
//! untouched as [`serde_json::Value`].

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::account::DeviceInfo;

/// A way to prove it is really you after the password.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecondFactor {
    /// Six digits from an authenticator app.
    Totp,
    /// One of the recovery codes shown when the app was set up.
    RecoveryCode,
    /// A passkey or security key.
    Passkey,
    /// Six digits sent by email to the account's confirmed address, once
    /// asked for with `/auth/second-factor/email`. Weaker than the others:
    /// whoever reads the mailbox has it.
    Email,
}

/// `POST /api/v1/auth/login` answers 202 with this when the password was
/// right but the account asks for a second factor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecondFactorNeeded {
    /// Single use, send it back with the factor.
    pub challenge: String,
    /// What the account can use.
    pub methods: Vec<SecondFactor>,
    /// The challenge stops working then, sign in again after.
    pub expires_at: i64,
    /// For `navigator.credentials.get`, when a passkey is among the methods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passkey: Option<Value>,
}

/// `POST /api/v1/auth/second-factor`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecondFactorRequest {
    /// From [`SecondFactorNeeded`].
    pub challenge: String,
    /// Which factor this is.
    pub method: SecondFactor,
    /// The six digits, or a recovery code.
    #[serde(default)]
    pub code: Option<String>,
    /// The answer of `navigator.credentials.get`.
    #[serde(default)]
    pub passkey: Option<Value>,
}

/// `POST /api/v1/auth/second-factor/email`: sends a code to the address of
/// the account signing in, when codes by email are its second step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailCodeRequest {
    /// From [`SecondFactorNeeded`].
    pub challenge: String,
}

/// `GET /api/v1/me/security`: how the account signs in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each says whether one way to sign in is set up"
)]
pub struct Security {
    /// The account has a password. Accounts made through single sign-on
    /// have none, they confirm changes by having signed in moments ago.
    pub has_password: bool,
    /// An authenticator app is set up.
    pub totp: bool,
    /// Recovery codes not used yet.
    pub recovery_codes_left: u32,
    /// Passkeys and security keys.
    pub passkeys: Vec<Passkey>,
    /// Single sign-on identities linked to the account.
    pub sso: Vec<SsoIdentity>,
    /// The confirmed email, for resets and notices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// The server sends email, an address can be added.
    pub email_available: bool,
    /// Codes by email are this account's second step, see
    /// [`SecondFactor::Email`].
    #[serde(default)]
    pub email_codes: bool,
    /// Set while the account waits for its deletion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_at: Option<i64>,
}

/// A registered passkey.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Passkey {
    /// Id.
    pub id: String,
    /// Name given when it was added.
    pub name: String,
    /// When it was added.
    pub created_at: i64,
    /// Last sign-in with it.
    pub last_used_at: Option<i64>,
}

/// A linked single sign-on identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoIdentity {
    /// Id, to unlink it.
    pub id: String,
    /// The provider's name for the account, an email or a username.
    pub name: String,
    /// When it was linked.
    pub created_at: i64,
}

/// Proof that the person at the keyboard owns the account, for changes to
/// how it signs in. The password, the proof of an OPAQUE sign-in for an
/// e2ee account, or nothing for an account without a password that signed
/// in during the last few minutes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Confirm {
    /// Current password.
    #[serde(default)]
    pub password: Option<String>,
    /// For an e2ee account, the proof from `/me/confirm` instead.
    #[serde(default)]
    pub proof: Option<String>,
}

/// `POST /api/v1/me/totp/setup`: a new secret, not active until confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TotpSetup {
    /// Base32 secret, for typing into the app by hand.
    pub secret: String,
    /// `otpauth://` link, what the QR code holds.
    pub uri: String,
    /// The QR code as an svg document.
    pub qr_svg: String,
}

/// `POST /api/v1/me/totp/confirm`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TotpConfirmRequest {
    /// Six digits from the app, proving it was set up right.
    pub code: String,
    /// See [`Confirm`].
    #[serde(default)]
    pub password: Option<String>,
    /// For an e2ee account, the proof from `/me/confirm` instead.
    #[serde(default)]
    pub proof: Option<String>,
}

/// New recovery codes, shown once. Each works one time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCodes {
    /// The codes.
    pub codes: Vec<String>,
}

/// A passkey ceremony in progress: `options` go to
/// `navigator.credentials.create` or `.get`, the answer comes back with `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyCeremony {
    /// Single use id of the ceremony.
    pub id: String,
    /// WebAuthn options.
    pub options: Value,
}

/// `POST /api/v1/me/passkeys/start`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyAddRequest {
    /// See [`Confirm`].
    #[serde(default)]
    pub password: Option<String>,
    /// For an e2ee account, the proof from `/me/confirm` instead.
    #[serde(default)]
    pub proof: Option<String>,
}

/// `POST /api/v1/me/passkeys/finish`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyFinishRequest {
    /// From [`PasskeyCeremony`].
    pub id: String,
    /// The answer of `navigator.credentials.create`.
    pub credential: Value,
    /// A name to recognise it, for example "Phone" or "Blue key".
    pub name: String,
}

/// A passkey just added. A first second factor also brings recovery
/// codes, shown once, empty otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyAdded {
    /// The passkey.
    pub passkey: Passkey,
    /// New recovery codes.
    pub codes: Vec<String>,
}

/// `POST /api/v1/auth/passkey/start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeySignInRequest {
    /// Username, any letter case. Empty: the browser offers the passkeys it
    /// holds for this server and the one picked names the account.
    #[serde(default)]
    pub username: String,
}

/// `POST /api/v1/auth/passkey/finish`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeySignInFinish {
    /// From [`PasskeyCeremony`].
    pub id: String,
    /// The answer of `navigator.credentials.get`.
    pub credential: Value,
    /// This device.
    pub device: DeviceInfo,
}

/// `POST /api/v1/auth/sso/start` and `/api/v1/me/sso/link`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoStartRequest {
    /// Path on this server to come back to, for example `/admin`. The
    /// sign-in code arrives in its fragment, `#sso=<code>`.
    pub return_to: String,
    /// For linking, see [`Confirm`].
    #[serde(default)]
    pub password: Option<String>,
    /// For an e2ee account, the proof from `/me/confirm` instead.
    #[serde(default)]
    pub proof: Option<String>,
}

/// Where to send the browser for single sign-on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoStart {
    /// The provider's sign-in page.
    pub url: String,
}

/// `POST /api/v1/auth/sso/exchange`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoExchangeRequest {
    /// The code from the fragment. It works once, for a minute.
    pub code: String,
    /// This device.
    pub device: DeviceInfo,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods_and_optional_fields_on_the_wire() {
        let needed = SecondFactorNeeded {
            challenge: "c".into(),
            methods: vec![SecondFactor::Totp, SecondFactor::RecoveryCode],
            expires_at: 1,
            passkey: None,
        };
        let json = serde_json::to_value(&needed).unwrap();
        assert_eq!(json["methods"][1], "recoveryCode");
        assert!(json.get("passkey").is_none());
        let req: SecondFactorRequest =
            serde_json::from_str(r#"{"challenge":"c","method":"totp","code":"123456"}"#).unwrap();
        assert_eq!(req.method, SecondFactor::Totp);
        assert!(req.passkey.is_none());
    }
}
