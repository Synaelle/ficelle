// SPDX-License-Identifier: AGPL-3.0-or-later

//! Signing in to a server from the app. A standard account sends its
//! password over TLS like the web pages do. An e2ee account runs OPAQUE
//! here, so the password never leaves the device, and opens the account
//! key with what the exchange yields. Either may then ask for a second
//! factor before the session comes. An e2ee account whose record still
//! stretches the password the old way is registered again with the same
//! password once signed in, see [`strengthen`].

use app_e2ee::{AccountKeys, KeyBundle, Ksf, MasterKey};
use app_protocol::account::{DeviceInfo, LoginRequest, Session};
use app_protocol::e2ee::{
    AccountKind, ConfirmFinish, E2eePasswordRequest, KindAnswer, KindRequest, OpaqueLoginFinish,
    OpaqueLoginStart, OpaqueLoginStarted, OpaqueRegistrationRequest, OpaqueRegistrationResponse,
    Proof,
};
use app_protocol::security::{SecondFactor, SecondFactorNeeded, SecondFactorRequest};
use serde::Serialize;
use zeroize::Zeroizing;

use super::http::{Api, HttpError};

/// This device as the server lists it.
pub fn device() -> DeviceInfo {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|name| !name.trim().is_empty());
    let platform = std::env::consts::OS;
    let name = match (host, platform) {
        (Some(host), _) => format!("{} on {host}", app_brand::DISPLAY_NAME),
        (None, "android") => format!("{} on Android", app_brand::DISPLAY_NAME),
        (None, _) => app_brand::DISPLAY_NAME.to_owned(),
    };
    // the app keeps its session, it has no earlier sign-in to replace
    DeviceInfo {
        name,
        platform: platform.to_owned(),
        replaces: None,
    }
}

/// A sign-in that needs a second factor, kept until it is given.
pub struct Pending {
    pub api: Api,
    pub challenge: String,
    /// E2ee: what opens the account key once the session comes.
    export_key: Option<Zeroizing<Vec<u8>>>,
    /// E2ee with an old record: what registers it again.
    stretch: Option<Stretch>,
}

/// An account whose record stretches the password less than this build
/// would: the password, kept until the session comes, to register again.
struct Stretch {
    username: String,
    password: Zeroizing<String>,
    /// How the record stretches it now.
    ksf: Ksf,
}

/// The end of a sign-in.
pub enum Outcome {
    Done(Signed),
    SecondFactor(Pending, Vec<SecondFactor>),
}

/// A session and, for e2ee, the account key.
pub struct Signed {
    pub session: Session,
    pub master_key: Option<MasterKey>,
}

/// What failed, for the sign-in screen.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInError {
    /// The server's error code, `offline`, or `keys` when an e2ee account
    /// key does not open.
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}

impl From<HttpError> for SignInError {
    fn from(err: HttpError) -> Self {
        let retry_after_seconds = match &err {
            HttpError::Api { body, .. } => body.retry_after_seconds,
            HttpError::Offline(_) | HttpError::Certificate(_) => None,
        };
        Self {
            code: err.code().to_owned(),
            message: err.to_string(),
            retry_after_seconds,
        }
    }
}

impl SignInError {
    fn keys(why: &impl std::fmt::Display) -> Self {
        Self {
            code: "keys".into(),
            message: format!("the account key does not open: {why}"),
            retry_after_seconds: None,
        }
    }
}

fn two_ways(
    api: &Api,
    answer: super::http::Answer,
    export_key: Option<Zeroizing<Vec<u8>>>,
    stretch: Option<Stretch>,
) -> Result<Outcome, SignInError> {
    if answer.status == 202 {
        let needed: SecondFactorNeeded = answer.parse()?;
        return Ok(Outcome::SecondFactor(
            Pending {
                api: api.clone(),
                challenge: needed.challenge,
                export_key,
                stretch,
            },
            needed.methods,
        ));
    }
    let session: Session = answer.parse()?;
    finish(
        api,
        session,
        export_key.as_deref().map(Vec::as_slice),
        stretch.as_ref(),
    )
    .map(Outcome::Done)
}

fn finish(
    api: &Api,
    session: Session,
    export_key: Option<&[u8]>,
    stretch: Option<&Stretch>,
) -> Result<Signed, SignInError> {
    let master_key = match (session.user.kind, export_key) {
        // signed in with OPAQUE, yet the server calls the account standard:
        // it would get everything sent from now on in plain text
        (AccountKind::Standard, Some(_)) => return Err(downgraded()),
        (AccountKind::Standard, None) => None,
        (AccountKind::E2ee, Some(export_key)) => {
            let keys = api.get("/me/keys", Some(&session.access_token))?;
            let bundle: KeyBundle = serde_json::from_value(keys.body["keys"].clone())
                .map_err(|e| SignInError::keys(&e))?;
            let master = AccountKeys::open_with_password(&bundle, export_key)
                .map_err(|e| SignInError::keys(&e))?;
            if let Some(stretch) = stretch
                && let Err(err) = strengthen(api, &session.access_token, &bundle, &master, stretch)
            {
                // the sign-in stands, the next one tries again
                eprintln!("strengthening the password: {}", err.message);
            }
            Some(master)
        }
        (AccountKind::E2ee, None) => {
            return Err(SignInError::keys(&"signed in without the password"));
        }
    };
    Ok(Signed {
        session,
        master_key,
    })
}

/// Registers the account's password again with [`Ksf::CURRENT`]: a proof
/// of the password as the record stands, a new record for the same
/// password, and the master key wrapped for its new export key. The
/// recovery wrapping and every device's session stay as they were.
fn strengthen(
    api: &Api,
    token: &str,
    bundle: &KeyBundle,
    master: &MasterKey,
    stretch: &Stretch,
) -> Result<(), SignInError> {
    let password = stretch.password.as_str();
    let start = app_e2ee::login_start(password).map_err(|e| SignInError::keys(&e))?;
    let started: OpaqueLoginStarted = api
        .post(
            "/me/confirm/start",
            Some(token),
            &OpaqueLoginStart {
                username: String::new(),
                request: start.request.clone(),
            },
        )?
        .parse()?;
    let confirmed = app_e2ee::login_finish(
        &start.state,
        password,
        &started.response,
        &stretch.username,
        stretch.ksf,
    )
    .map_err(|e| SignInError::keys(&e))?;
    let proof: Proof = api
        .post(
            "/me/confirm/finish",
            Some(token),
            &ConfirmFinish {
                challenge: started.challenge,
                finalization: confirmed.finalization,
            },
        )?
        .parse()?;
    let start = app_e2ee::registration_start(password).map_err(|e| SignInError::keys(&e))?;
    let answer: OpaqueRegistrationResponse = api
        .post(
            "/auth/opaque/registration",
            None,
            &OpaqueRegistrationRequest {
                username: stretch.username.clone(),
                request: start.request.clone(),
            },
        )?
        .parse()?;
    let made = app_e2ee::registration_finish(
        &start.state,
        password,
        &answer.response,
        &stretch.username,
        Ksf::CURRENT,
    )
    .map_err(|e| SignInError::keys(&e))?;
    let keys = AccountKeys::rewrap_password(bundle, master, &made.export_key);
    api.post(
        "/me/e2ee/strengthen",
        Some(token),
        &E2eePasswordRequest {
            upload: made.upload,
            keys: app_protocol::e2ee::KeyBundle {
                version: keys.version,
                by_password: keys.by_password,
                by_recovery: keys.by_recovery,
            },
            proof: proof.proof,
            ksf: Ksf::CURRENT.number(),
        },
    )?;
    Ok(())
}

/// A server whose account record this build cannot open.
fn update_needed() -> SignInError {
    SignInError {
        code: "updateApp".into(),
        message:
            "this server signs in in a way this version of the app does not know, update the app"
                .into(),
        retry_after_seconds: None,
    }
}

/// A server calling an account standard that this vault knows as e2ee.
fn downgraded() -> SignInError {
    SignInError {
        code: "downgraded".into(),
        message: "this server says the account is no longer end-to-end encrypted, which it cannot become: the password is not sent".into(),
        retry_after_seconds: None,
    }
}

/// Signs in with a username and password, whichever kind the account is.
/// `known_e2ee`: this vault signed in to the account as e2ee before. The
/// server's word that it is standard is then refused, the password is never
/// sent in a form it can read.
pub fn sign_in(
    api: &Api,
    username: &str,
    password: &str,
    known_e2ee: bool,
) -> Result<Outcome, SignInError> {
    let kind: KindAnswer = api
        .post(
            "/auth/kind",
            None,
            &KindRequest {
                username: username.to_owned(),
            },
        )?
        .parse()?;
    match kind.kind {
        AccountKind::Standard if known_e2ee => Err(downgraded()),
        AccountKind::Standard => {
            let answer = api.post(
                "/auth/login",
                None,
                &LoginRequest {
                    username: username.to_owned(),
                    password: password.to_owned(),
                    device: device(),
                },
            )?;
            two_ways(api, answer, None, None)
        }
        AccountKind::E2ee => {
            let start = app_e2ee::login_start(password).map_err(|e| SignInError::keys(&e))?;
            // a number this build does not know: a newer server's record
            let started: OpaqueLoginStarted = api
                .post(
                    "/auth/opaque/login/start",
                    None,
                    &OpaqueLoginStart {
                        username: username.to_owned(),
                        request: start.request.clone(),
                    },
                )?
                .parse()?;
            let ksf = Ksf::from_number(started.ksf).ok_or_else(update_needed)?;
            // a wrong password already fails here: the server still gets an
            // answer, so it counts the failure and replies as for any wrong
            // password
            let (finalization, export_key) = match app_e2ee::login_finish(
                &start.state,
                password,
                &started.response,
                username,
                ksf,
            ) {
                Ok(done) => (
                    done.finalization,
                    Some(Zeroizing::new(done.export_key.to_vec())),
                ),
                Err(_) => (app_e2ee::encode(&[0u8; 64]), None),
            };
            let stretch = (ksf < Ksf::CURRENT && export_key.is_some()).then(|| Stretch {
                username: username.to_owned(),
                password: Zeroizing::new(password.to_owned()),
                ksf,
            });
            let answer = api.post(
                "/auth/opaque/login/finish",
                None,
                &OpaqueLoginFinish {
                    challenge: started.challenge,
                    finalization,
                    device: device(),
                },
            )?;
            two_ways(api, answer, export_key, stretch)
        }
    }
}

/// Finishes a sign-in with the second factor.
pub fn second_factor(
    pending: &Pending,
    method: SecondFactor,
    code: &str,
) -> Result<Signed, SignInError> {
    let session: Session = pending
        .api
        .post(
            "/auth/second-factor",
            None,
            &SecondFactorRequest {
                challenge: pending.challenge.clone(),
                method,
                code: Some(code.trim().to_owned()),
                passkey: None,
            },
        )?
        .parse()?;
    finish(
        &pending.api,
        session,
        pending.export_key.as_deref().map(Vec::as_slice),
        pending.stretch.as_ref(),
    )
}
