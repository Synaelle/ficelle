// SPDX-License-Identifier: AGPL-3.0-or-later

//! Feedback and bug reports from this device. Where they go: signed in to a
//! server, where its admin says (the main instance, that server, or
//! nowhere), otherwise the main instance (`brand.json` `feedbackUrl`). No
//! account is used to send, the report is anonymous wherever it goes. The
//! proof of work the server asks for is done here, on a blocking thread.

use std::time::Instant;

use app_protocol::feedback::{Challenge, FeedbackInfo, Report, SendTo};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::http::{Api, HttpError};
use super::sharing::{authed, blocking, failed};
use super::sign_in::SignInError;
use crate::state::AppState;

/// The main instance's address. Dev builds may point at a test server, as
/// they may for the catalog: release builds always use the brand's.
fn main_instance() -> String {
    cfg!(debug_assertions)
        .then(|| std::env::var("APP_FEEDBACK_URL").ok())
        .flatten()
        .unwrap_or_else(|| app_brand::FEEDBACK_URL.to_owned())
}

/// Where this device's reports go.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackWhere {
    /// Send feedback is offered.
    available: bool,
    /// The server that receives, as an address.
    url: String,
    /// Its host, named on the form.
    host: String,
    /// `main` (the project's), `server` (the one signed in to), or why
    /// there is none: `off` (the admin turned it off), `none` (this build
    /// has no main instance).
    via: &'static str,
    /// What a report says of this app, shown before sending.
    version: &'static str,
    platform: &'static str,
}

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn resolve(state: &AppState) -> Result<FeedbackWhere, SignInError> {
    let main = main_instance();
    let to_main = || FeedbackWhere {
        available: !main.is_empty(),
        host: host_of(&main),
        url: main.clone(),
        via: if main.is_empty() { "none" } else { "main" },
        version: app_brand::VERSION,
        platform: platform(),
    };
    // a locked or missing vault is no server: the main instance
    let link = state.quietly(|s| s.store.sync_link()).ok().flatten();
    let Some(link) = link.filter(|l| l.tokens.is_some()) else {
        return Ok(to_main());
    };
    let api = Api::new(&link.server);
    let info: FeedbackInfo = match api
        .get("/feedback/info", None)
        .and_then(super::http::Answer::parse)
    {
        Ok(info) => info,
        // a server from before feedback existed says nothing: the default
        Err(HttpError::Api { status: 404, .. }) => FeedbackInfo {
            receive: false,
            send_to: SendTo::Main,
        },
        Err(err) => return Err(err.into()),
    };
    Ok(match info.send_to {
        SendTo::Main => to_main(),
        SendTo::Here => FeedbackWhere {
            available: true,
            host: host_of(api.base()),
            url: api.base().to_owned(),
            via: "server",
            version: app_brand::VERSION,
            platform: platform(),
        },
        SendTo::Off => FeedbackWhere {
            available: false,
            host: String::new(),
            url: String::new(),
            via: "off",
            version: app_brand::VERSION,
            platform: platform(),
        },
    })
}

/// Where a report would go now.
#[tauri::command]
pub async fn feedback_where(app: AppHandle) -> Result<FeedbackWhere, SignInError> {
    blocking(app, resolve).await
}

/// A report as the form gives it. The version and platform are this app's.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    install: String,
    /// What it is about, in one line.
    about: String,
    #[serde(default)]
    message: String,
    /// An email address or a Discord name.
    #[serde(default)]
    contact: Option<String>,
    #[serde(default)]
    logs: Option<String>,
    #[serde(default)]
    images: Vec<String>,
    /// SHA-256 hex of a secret the page keeps, to ask later how the report
    /// stands ([`feedback_answers`]).
    #[serde(default)]
    receipt: Option<String>,
}

/// What a sent report cost.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    host: String,
    /// The instance it went to, where its answer is asked.
    url: String,
    /// The work asked, in zero bits, and the time it took here.
    bits: u8,
    work_ms: u64,
}

/// The app's platform as the server stores it.
pub fn platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        _ => "other",
    }
}

/// Sends to `url`. `server` is the host of the server this app is signed
/// in to, for the receiving instance's caps; `token` its sign-in, given
/// only to that same server, whose own people are not capped.
fn send(
    url: &str,
    draft: Draft,
    server: Option<String>,
    token: Option<&str>,
) -> Result<Sent, SignInError> {
    let api = Api::new(url);
    let challenge: Challenge = api
        .post("/feedback/challenge", None, &serde_json::json!({}))?
        .parse()?;
    let started = Instant::now();
    let nonce = app_protocol::feedback::solve(&challenge.challenge, challenge.bits);
    let work_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let report = Report {
        challenge: challenge.challenge,
        nonce,
        install: Some(draft.install),
        category: None,
        about: Some(draft.about),
        message: draft.message,
        contact: draft.contact.filter(|c| !c.trim().is_empty()),
        version: app_brand::VERSION.to_owned(),
        platform: platform().to_owned(),
        logs: draft.logs.filter(|l| !l.trim().is_empty()),
        images: draft.images,
        server,
        receipt: draft.receipt,
    };
    api.post("/feedback", token, &report)?;
    Ok(Sent {
        host: host_of(url),
        url: url.to_owned(),
        bits: challenge.bits,
        work_ms,
    })
}

/// Sends a report where [`feedback_where`] says.
#[tauri::command]
pub async fn feedback_send(app: AppHandle, draft: Draft) -> Result<Sent, SignInError> {
    blocking(app, move |state| {
        let place = resolve(state)?;
        if !place.available {
            return Err(failed("feedbackOff", "feedback is turned off here"));
        }
        let link = state.quietly(|s| s.store.sync_link()).ok().flatten();
        let server = link
            .as_ref()
            .filter(|l| l.tokens.is_some())
            .map(|l| host_of(&l.server));
        if server.is_some() && server.as_deref() == Some(host_of(&place.url).as_str()) {
            // the server signed in to: a fresh token shows the report comes
            // from one of its people
            let token = authed(state, |api, token| {
                api.get("/me", Some(token)).map(|_| token.to_owned())
            })
            .ok();
            return send(&place.url, draft, server, token.as_deref());
        }
        send(&place.url, draft, server, None)
    })
    .await
}

/// How the reports sent to `url` with these secrets stand. Only an
/// instance this app sends feedback to is asked.
#[tauri::command]
pub async fn feedback_answers(
    app: AppHandle,
    url: String,
    secrets: Vec<String>,
) -> Result<app_protocol::feedback::Answers, SignInError> {
    blocking(app, move |_state| {
        Api::new(&url)
            .post(
                "/feedback/answers",
                None,
                &app_protocol::feedback::AnswersRequest { secrets },
            )?
            .parse()
            .map_err(Into::into)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_named_without_scheme_or_path() {
        assert_eq!(
            host_of("https://ficelle.synaelle.com"),
            "ficelle.synaelle.com"
        );
        assert_eq!(host_of("http://127.0.0.1:8631/"), "127.0.0.1:8631");
        assert_eq!(host_of(""), "");
    }
}
