// SPDX-License-Identifier: AGPL-3.0-or-later

//! Types exchanged between clients and the server.
//!
//! Every request carries the client protocol version and the server lists the
//! versions it accepts, so a mismatch becomes a clear "please update" message
//! instead of a silent failure.

use serde::{Deserialize, Serialize};

/// Protocol version spoken by this build. Bump on any breaking wire change.
/// 2 since 0.6.0: folders became nested tags, and an app from before them
/// would file notes in folders the others no longer read, or trash a
/// folder's notes along with it.
pub const PROTOCOL_VERSION: u32 = 2;

/// Oldest protocol version this build still understands.
pub const MIN_PROTOCOL_VERSION: u32 = 2;

/// Oldest protocol feedback reports still come from: their format did not
/// change, an app too old to sync can still send one.
pub const MIN_FEEDBACK_PROTOCOL: u32 = 1;

/// Direct sync between a person's devices on the same network.
pub mod local {
    /// Version of what two devices say to each other. Bump on any breaking
    /// change of the messages.
    pub const PROTOCOL: u32 = 1;
    /// Oldest version this build still speaks.
    pub const MIN_PROTOCOL: u32 = 1;
    /// Shape of the documents. Grows when an older app would misread them:
    /// two devices sync only on the same level, the older one asks for an
    /// update.
    pub const DATA_FORMAT: u32 = 1;
}

/// How far two clocks may disagree before a device refuses to sync, in
/// milliseconds. Clocks set by the network agree within seconds: this only
/// catches a clock set wrong, whose dates would make trash expire early on
/// the devices it syncs with, or never.
pub const CLOCK_TOLERANCE_MS: i64 = 10 * 60 * 1000;

/// How far this device's clock is off from another's, `None` within
/// [`CLOCK_TOLERANCE_MS`]. `before` and `after` are this device's times
/// just before asking and just after the answer, `theirs` the other's time
/// in the answer, taken in between and rounded down to `grain` (1000 for
/// an HTTP `Date`). Positive when this device is ahead.
pub fn clock_off(before: i64, after: i64, theirs: i64, grain: i64) -> Option<i64> {
    if theirs + grain < before - CLOCK_TOLERANCE_MS {
        Some(before - theirs)
    } else if theirs > after + CLOCK_TOLERANCE_MS {
        Some(after - theirs)
    } else {
        None
    }
}

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
    /// The server cannot do this for an end-to-end encrypted account, for
    /// example set its password.
    pub const E2EE_ACCOUNT: &str = "e2eeAccount";
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
    /// An app linking with a code: not approved yet, ask again later.
    pub const LINK_PENDING: &str = "linkPending";
}

pub mod account;
pub mod admin;
pub mod e2ee;
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
    /// The server has a sign-in page apps open in the browser (`/connect`,
    /// see [`account::AppCodeRequest`]). Servers before 0.6.0-beta.6 leave
    /// it out.
    #[serde(default)]
    pub sign_in_page: bool,
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
            sign_in_page: true,
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
    fn clocks_count_as_off_only_past_the_tolerance() {
        const MIN: i64 = 60_000;
        let (before, after) = (1_000_000_000_000, 1_000_000_000_400);
        // inside, the answer's time rounded down a second
        assert_eq!(clock_off(before, after, before - 1000, 1000), None);
        assert_eq!(clock_off(before, after, after, 0), None);
        assert_eq!(clock_off(before, after, before - 9 * MIN, 0), None);
        assert_eq!(clock_off(before, after, after + 9 * MIN, 0), None);
        assert_eq!(
            clock_off(before, after, before - CLOCK_TOLERANCE_MS - 1000, 1000),
            None
        );
        assert_eq!(
            clock_off(before, after, after + CLOCK_TOLERANCE_MS, 0),
            None
        );
        // this device ahead: the other's time is earlier
        assert_eq!(
            clock_off(before, after, before - 31 * 24 * 60 * MIN, 1000),
            Some(31 * 24 * 60 * MIN)
        );
        assert_eq!(
            clock_off(before, after, before - CLOCK_TOLERANCE_MS - 1, 0),
            Some(CLOCK_TOLERANCE_MS + 1)
        );
        // this device behind
        assert_eq!(
            clock_off(before, after, after + 2 * 60 * MIN, 0),
            Some(-2 * 60 * MIN)
        );
        assert_eq!(
            clock_off(before, after, after + CLOCK_TOLERANCE_MS + 1, 0),
            Some(-CLOCK_TOLERANCE_MS - 1)
        );
    }

    #[test]
    fn serializes_with_camel_case_keys() {
        let json = serde_json::to_value(info()).unwrap();
        assert_eq!(json["minProtocol"], 2);
        assert_eq!(json["maxProtocol"], 4);
    }
}
