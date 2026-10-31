// SPDX-License-Identifier: AGPL-3.0-or-later

//! The first start of a server: the setup link from the log is traded for a
//! setup session, the admin sets up a passkey or an authenticator app, and
//! `finish` makes the admin and locks setup for good.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::account::{DeviceInfo, Registration, Session};
use crate::e2ee::AccountKinds;

/// `GET /api/v1/setup`: what the setup page needs before the link is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    /// This request passes the https rule, so the link may be used from here.
    pub secure: bool,
    /// Passkeys work on this server (it knows its public address).
    pub passkeys: bool,
    /// The address the server believes it is reached at, empty when unset.
    pub public_url: String,
}

/// `POST /api/v1/setup/start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStartRequest {
    /// The token after `#token=` in the setup link.
    pub token: String,
}

/// A setup session, good for an hour. The link no longer works.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStarted {
    /// Sent with every further setup call.
    pub session: String,
    /// End of the session, unix milliseconds.
    pub expires_at: i64,
}

/// `POST /api/v1/setup/totp` and `/api/v1/setup/passkey`: the second factor
/// of the admin about to be made.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupFactorRequest {
    /// From [`SetupStarted`].
    pub session: String,
    /// The admin's username, shown by the authenticator.
    pub username: String,
    /// Name shown to others, the username when absent.
    #[serde(default)]
    pub display_name: Option<String>,
}

/// The second factor proven at `finish`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SetupFactor {
    /// The answer of `navigator.credentials.create` to the options of
    /// `/setup/passkey`.
    Passkey {
        /// The credential.
        credential: Value,
        /// A name to recognise it.
        #[serde(default)]
        name: String,
    },
    /// A code from the app set up with `/setup/totp`.
    Totp {
        /// Six digits.
        code: String,
    },
    /// No second step: the admin signs in with the password alone, and
    /// admins no longer need one ([`crate::admin::Overview`]'s
    /// `admins_need_second_step` off).
    None,
}

/// `POST /api/v1/setup/finish`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupFinishRequest {
    /// From [`SetupStarted`].
    pub session: String,
    /// Admin username.
    pub username: String,
    /// Name shown to others, the username when absent.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Admin password.
    pub password: String,
    /// The second factor, or none chosen past a warning.
    pub factor: SetupFactor,
    /// The server is for its admin alone ("Sync for myself"): the admin
    /// page then leaves out what is about other people.
    #[serde(default)]
    pub personal: bool,
    /// Who may make an account, closed when absent.
    #[serde(default)]
    pub registration: Option<Registration>,
    /// Which kinds of accounts sign-up makes, unchanged when absent.
    #[serde(default)]
    pub account_kinds: Option<AccountKinds>,
    /// This device.
    pub device: DeviceInfo,
}

/// The admin made and signed in. Recovery codes for a lost factor, shown once.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupFinished {
    /// The admin's session.
    pub session: Session,
    /// Recovery codes.
    pub recovery_codes: Vec<String>,
}

/// `POST /api/v1/setup/checks`: what the server sees of the request, for the
/// health checks the setup page shows. The page adds what only it can
/// test: a WebSocket and an upload through the proxy, and its own clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts the page reports one by one, not a state"
)]
pub struct SetupChecks {
    /// The request passes the https rule.
    pub secure: bool,
    /// It came through a trusted proxy.
    pub via_proxy: bool,
    /// That proxy said who the client is (`X-Forwarded-For`).
    pub forwarded_for: bool,
    /// That proxy said the scheme (`X-Forwarded-Proto`), lower case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forwarded_proto: Option<String>,
    /// The client's address as the server believes it.
    pub client_ip: String,
    /// The host the request named, `X-Forwarded-Host` from the proxy or `Host`.
    pub host: String,
    /// `server.public_url`, empty when unset.
    pub public_url: String,
    /// The host of `public_url` is the one the request named.
    pub host_matches: bool,
    /// The server's clock, unix milliseconds.
    pub server_time: i64,
    /// The largest upload part a device sends, what the proxy must let pass.
    pub part_bytes: u64,
    /// Email is set up in `[smtp]`.
    pub mail: bool,
}
