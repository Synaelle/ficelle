// SPDX-License-Identifier: AGPL-3.0-or-later

//! Types exchanged between clients and the server.
//!
//! Every request carries the client protocol version and the server lists the
//! versions it accepts, so a mismatch becomes a clear "please update" message
//! instead of a silent failure.

use serde::{Deserialize, Serialize};

/// Protocol version spoken by this build. Bump on any breaking wire change.
pub const PROTOCOL_VERSION: u32 = 1;

/// Oldest protocol version this build still understands.
pub const MIN_PROTOCOL_VERSION: u32 = 1;

/// Request header carrying the client's protocol version. Every api call
/// sends it, except `GET /api/v1/info`, which a client reads first to learn
/// whether it can talk to the server at all.
pub const PROTOCOL_HEADER: &str = "x-protocol-version";

/// Body of every failed api call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    /// Stable code the client translates, see [`error_code`].
    pub error: String,
    /// English detail for logs and people reading raw responses.
    pub message: String,
    /// For `lockedOut` and `rateLimited`: seconds before trying again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}

/// Error codes shared by the clients and the server.
pub mod error_code {
    /// The request has no valid [`super::PROTOCOL_HEADER`].
    pub const PROTOCOL_MISSING: &str = "protocolMissing";
    /// The app is older than the server accepts: update the app.
    pub const UPDATE_APP: &str = "updateApp";
    /// The app is newer than the server knows: the server needs an update.
    pub const UPDATE_SERVER: &str = "updateServer";
    /// A backup could not be made, read or restored: the message says why.
    pub const BACKUP_FAILED: &str = "backupFailed";
    /// No such endpoint.
    pub const NOT_FOUND: &str = "notFound";
    /// Something failed on the server, details are in its log.
    pub const INTERNAL: &str = "internal";
    /// The request body is not what the endpoint expects.
    pub const INVALID: &str = "invalid";
    /// Wrong username or password. Never says which.
    pub const BAD_CREDENTIALS: &str = "badCredentials";
    /// Too many failed sign-ins, wait `retryAfterSeconds`.
    pub const LOCKED_OUT: &str = "lockedOut";
    /// Too many requests from this address, wait a moment.
    pub const RATE_LIMITED: &str = "rateLimited";
    /// An admin turned this account off.
    pub const ACCOUNT_DISABLED: &str = "accountDisabled";
    /// No valid access token: sign in, or refresh.
    pub const UNAUTHORIZED: &str = "unauthorized";
    /// The refresh token was used twice, the device is signed out everywhere.
    pub const SESSION_REVOKED: &str = "sessionRevoked";
    /// Signed in, but not allowed to do this.
    pub const FORBIDDEN: &str = "forbidden";
    /// Registration needs an invite, or is closed.
    pub const REGISTRATION_CLOSED: &str = "registrationClosed";
    /// The invite is unknown, used, revoked or expired.
    pub const INVITE_INVALID: &str = "inviteInvalid";
    /// Someone already has this username.
    pub const USERNAME_TAKEN: &str = "usernameTaken";
    /// Username outside 3 to 32 of `a-z 0-9 . _ -`.
    pub const USERNAME_INVALID: &str = "usernameInvalid";
    /// Password shorter than 8 characters, or the username itself.
    pub const PASSWORD_WEAK: &str = "passwordWeak";
    /// The first admin already exists.
    pub const SETUP_DONE: &str = "setupDone";
    /// The setup link is wrong, used or expired, or its session ended.
    pub const SETUP_LINK_INVALID: &str = "setupLinkInvalid";
    /// The server is waiting for its first admin, nothing else answers yet.
    pub const SETUP_PENDING: &str = "setupPending";
    /// Setup runs over https, or from the server itself.
    pub const HTTPS_REQUIRED: &str = "httpsRequired";
    /// An admin made or reset from the command line adds a passkey or an
    /// authenticator app before the admin pages answer it.
    pub const SECOND_FACTOR_REQUIRED: &str = "secondFactorRequired";
    /// The mail server refused or could not be reached, the message says why.
    pub const MAIL_FAILED: &str = "mailFailed";
    /// The change would leave the server without an active admin.
    pub const LAST_ADMIN: &str = "lastAdmin";
    /// Another group has this name.
    pub const GROUP_NAME_TAKEN: &str = "groupNameTaken";
    /// The second factor is wrong, or the challenge expired or was used up.
    pub const SECOND_FACTOR_INVALID: &str = "secondFactorInvalid";
    /// The code from the authenticator app is wrong.
    pub const TOTP_INVALID: &str = "totpInvalid";
    /// The change needs the current password, or a fresh sign-in.
    pub const CONFIRM_NEEDED: &str = "confirmNeeded";
    /// The passkey answer does not check out.
    pub const PASSKEY_INVALID: &str = "passkeyInvalid";
    /// Passkeys need `server.public_url`, which is not set.
    pub const PASSKEYS_UNAVAILABLE: &str = "passkeysUnavailable";
    /// Single sign-on is not set up on this server.
    pub const SSO_UNAVAILABLE: &str = "ssoUnavailable";
    /// The provider refused, or its answer does not check out.
    pub const SSO_FAILED: &str = "ssoFailed";
    /// No account is linked to this identity and the server does not create one.
    pub const SSO_NO_ACCOUNT: &str = "ssoNoAccount";
    /// This identity is already linked to an account.
    pub const SSO_TAKEN: &str = "ssoTaken";
    /// The server cannot do this for an end-to-end encrypted account, for
    /// example set its password.
    pub const E2EE_ACCOUNT: &str = "e2eeAccount";
    /// This server does not create accounts of this kind.
    pub const ACCOUNT_KIND_CLOSED: &str = "accountKindClosed";
    /// The recovery code is wrong, or the recovery took too long.
    pub const RECOVERY_INVALID: &str = "recoveryInvalid";
    /// The server sends no email, `[smtp]` is not set up.
    pub const MAIL_OFF: &str = "mailOff";
    /// Not an email address.
    pub const EMAIL_INVALID: &str = "emailInvalid";
    /// The link is used, expired, or was never sent.
    pub const LINK_INVALID: &str = "linkInvalid";
    /// A sync change is not usable: unknown document id, bad data, too big.
    pub const CHANGE_INVALID: &str = "changeInvalid";
    /// A file is larger than the account may store.
    pub const FILE_TOO_LARGE: &str = "fileTooLarge";
    /// The account's storage is full.
    pub const QUOTA_FULL: &str = "quotaFull";
    /// This type of file is not allowed.
    pub const FILE_TYPE_REFUSED: &str = "fileTypeRefused";
    /// A file upload that does not fit: a part out of order, content that
    /// does not match its address.
    pub const FILE_INVALID: &str = "fileInvalid";
    /// An upload part out of place: another device sends the same file, or
    /// an answer was lost. The device asks where the upload stands, later.
    pub const UPLOAD_MOVED: &str = "uploadMoved";
    /// The space changed since the device last looked: it syncs, then
    /// tries again.
    pub const SPACE_MOVED_ON: &str = "spaceMovedOn";
    /// This instance does not take feedback reports.
    pub const FEEDBACK_OFF: &str = "feedbackOff";
    /// The report's challenge is unknown, used, expired or not answered.
    pub const CHALLENGE_INVALID: &str = "challengeInvalid";
}

pub mod account;
pub mod admin;
pub mod catalog;
pub mod e2ee;
pub mod extensions;
pub mod feedback;
pub mod files;
pub mod profile;
pub mod releases;
pub mod security;
pub mod setup;
pub mod sync;

/// Whether an app and a server can talk, and who has to update otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    /// Same language spoken.
    Compatible,
    /// The app is too old for this server.
    UpdateApp,
    /// The server is too old for this app.
    UpdateServer,
}

/// Public description of a server, returned by `GET /api/v1/info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// Product name of the server software.
    pub name: String,
    /// Server software version.
    pub version: String,
    /// Lowest protocol version the server accepts.
    pub min_protocol: u32,
    /// Highest protocol version the server accepts.
    pub max_protocol: u32,
}

impl ServerInfo {
    /// Tells whether a client speaking `version` can talk to this server.
    pub fn supports(&self, version: u32) -> bool {
        (self.min_protocol..=self.max_protocol).contains(&version)
    }

    /// What a client speaking `version` should tell its user.
    pub fn compatibility(&self, version: u32) -> Compatibility {
        if version < self.min_protocol {
            Compatibility::UpdateApp
        } else if version > self.max_protocol {
            Compatibility::UpdateServer
        } else {
            Compatibility::Compatible
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ServerInfo {
        ServerInfo {
            name: "server".into(),
            version: "0.1.0".into(),
            min_protocol: 2,
            max_protocol: 4,
        }
    }

    #[test]
    fn supports_only_the_advertised_range() {
        let info = info();
        assert!(!info.supports(1));
        assert!(info.supports(2));
        assert!(info.supports(4));
        assert!(!info.supports(5));
    }

    #[test]
    fn says_which_side_has_to_update() {
        let info = info();
        assert_eq!(info.compatibility(1), Compatibility::UpdateApp);
        assert_eq!(info.compatibility(3), Compatibility::Compatible);
        assert_eq!(info.compatibility(5), Compatibility::UpdateServer);
    }

    #[test]
    fn serializes_with_camel_case_keys() {
        let json = serde_json::to_value(info()).unwrap();
        assert_eq!(json["minProtocol"], 2);
        assert_eq!(json["maxProtocol"], 4);
    }
}
