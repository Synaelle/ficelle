// SPDX-License-Identifier: AGPL-3.0-or-later

//! The first start through the full router: the link, setup pending, the
//! https rule, the mandatory second factor, the standard or end-to-end
//! encrypted admin and the lock.

use std::net::SocketAddr;

use app_protocol::{PROTOCOL_HEADER, PROTOCOL_VERSION};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use base64::Engine;
use serde_json::{Value, json};
use tower::ServiceExt;
use webauthn_authenticator_rs::WebauthnAuthenticator;
use webauthn_authenticator_rs::softpasskey::SoftPasskey;
use webauthn_rs::prelude::Url;

use super::tests::{call, device, events, login};
use crate::accounts::totp;
use crate::clock::MINUTE;
use crate::config::{AuthConfig, Config};
use crate::db::Db;
use crate::routes::{AppState, router};

const ORIGIN: &str = "https://notes.example.org";
const PASSWORD: &str = "a long admin password";

/// A server that was never set up. Plain http is allowed unless `strict`,
/// requests in tests come from no address at all.
fn fresh(strict: bool, public_url: &str) -> (AppState, Router) {
    let mut config = Config {
        auth: AuthConfig {
            argon2_memory_kib: 64,
            argon2_iterations: 1,
            ..AuthConfig::default()
        },
        ..Config::default()
    };
    config.setup.allow_http = !strict;
    config.server.public_url = public_url.into();
    let state = AppState::for_tests(config, Db::in_memory().unwrap());
    let app = router(state.clone());
    (state, app)
}

fn link(state: &AppState) -> String {
    state.setup.new_link(state.clock.now())
}

async fn start(app: &Router, token: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/setup/start",
        None,
        Some(json!({ "token": token })),
    )
    .await
}

async fn session(state: &AppState, app: &Router) -> String {
    let (status, body) = start(app, &link(state)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["session"].as_str().unwrap().to_owned()
}

/// Sets up the authenticator app, returns its secret.
async fn totp_secret(app: &Router, session: &str) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/setup/totp",
        None,
        Some(json!({ "session": session, "username": "Root" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["qrSvg"].as_str().unwrap().contains("<svg"));
    body["secret"].as_str().unwrap().to_owned()
}

fn finish_body(session: &str, factor: &Value) -> Value {
    json!({
        "session": session,
        "username": "Root",
        "password": PASSWORD,
        "factor": factor,
        "device": device("laptop"),
    })
}

async fn finish(app: &Router, session: &str, factor: &Value) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/setup/finish",
        None,
        Some(finish_body(session, factor)),
    )
    .await
}

#[tokio::test]
async fn until_setup_only_the_setup_and_first_look_answer() {
    let (_, app) = fresh(false, "");
    for path in ["/me", "/feedback/info", "/sync", "/admin/overview"] {
        let (status, body) = call(&app, "GET", path, None, None).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (StatusCode::SERVICE_UNAVAILABLE, Some("setupPending")),
            "{path}"
        );
    }
    let (status, body) = call(
        &app,
        "POST",
        "/auth/login",
        None,
        Some(json!({ "username": "a", "password": "b", "device": device("x") })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("setupPending"))
    );
    let (status, options) = call(&app, "GET", "/auth/options", None, None).await;
    assert_eq!(
        (status, &options["setupNeeded"]),
        (StatusCode::OK, &json!(true))
    );
    let (status, body) = call(&app, "GET", "/setup", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["passkeys"], false);
    let health = app
        .clone()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
}

#[tokio::test]
async fn the_link_works_once_and_wrong_ones_are_logged() {
    let (state, app) = fresh(false, "");
    let (status, body) = start(&app, "sl_guessed").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("setupLinkInvalid"))
    );
    let token = link(&state);
    let (status, body) = start(&app, &format!(" {token}\n")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["session"].as_str().unwrap().starts_with("su_"));
    let (status, _) = start(&app, &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "used");

    let old = link(&state);
    state.clock.advance(crate::setup::LIFE);
    let (status, _) = start(&app, &old).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "expired");
    let log = events(&state).await;
    assert_eq!(
        log.iter().filter(|e| *e == "setup_failed").count(),
        3,
        "{log:?}"
    );
    assert!(log.iter().any(|e| e == "setup_started"));
}

/// A request from `peer` with these headers, as the network delivers it.
async fn from(app: &Router, peer: &str, headers: &[(&str, &str)], body: &Value) -> StatusCode {
    let mut req = Request::post("/api/v1/setup/start")
        .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
        .header("content-type", "application/json");
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    let mut req = req.body(Body::from(body.to_string())).unwrap();
    let addr = SocketAddr::new(peer.parse().unwrap(), 40_000);
    req.extensions_mut().insert(ConnectInfo(addr));
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    if status == StatusCode::FORBIDDEN {
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "httpsRequired");
    }
    status
}

#[tokio::test]
async fn setup_runs_over_https_or_on_the_server_itself() {
    let (state, app) = fresh(true, ORIGIN);
    let body = json!({ "token": link(&state) });
    let forwarded = |proto| {
        [
            ("x-forwarded-for", "203.0.113.5"),
            ("x-forwarded-proto", proto),
        ]
    };
    assert_eq!(
        from(&app, "203.0.113.5", &[], &body).await,
        StatusCode::FORBIDDEN,
        "straight from the internet over http"
    );
    assert_eq!(
        from(&app, "127.0.0.1", &forwarded("http"), &body).await,
        StatusCode::FORBIDDEN,
        "through the proxy over http"
    );
    assert_eq!(
        from(&app, "203.0.113.5", &forwarded("https"), &body).await,
        StatusCode::FORBIDDEN,
        "https claimed by someone who is not the proxy"
    );
    assert_eq!(
        from(&app, "127.0.0.1", &forwarded("https"), &body).await,
        StatusCode::OK
    );
    let body = json!({ "token": link(&state) });
    assert_eq!(
        from(&app, "::1", &[], &body).await,
        StatusCode::OK,
        "on the server itself"
    );
}

#[tokio::test]
async fn an_authenticator_app_admin_and_the_lock() {
    let (state, app) = fresh(false, "");
    let session = session(&state, &app).await;
    let (status, body) = finish(&app, &session, &json!({ "type": "totp", "code": "123456" })).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("totpInvalid")),
        "no app set up yet"
    );
    let secret = totp_secret(&app, &session).await;
    let (status, body) = call(
        &app,
        "POST",
        "/setup/finish",
        None,
        Some(json!({
            "session": session, "username": "root", "password": "root",
            "factor": { "type": "totp", "code": totp::code_now(&secret, state.clock.now()) },
            "device": device("laptop"),
        })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("passwordWeak"))
    );
    let code = totp::code_now(&secret, state.clock.now());
    let (status, done) = finish(&app, &session, &json!({ "type": "totp", "code": code })).await;
    assert_eq!(status, StatusCode::CREATED, "{done}");
    assert_eq!(done["session"]["user"]["username"], "root");
    assert_eq!(done["session"]["user"]["role"], "admin");
    assert_eq!(done["recoveryCodes"].as_array().unwrap().len(), 10);
    let token = done["session"]["accessToken"].as_str().unwrap();
    let (status, security) = call(&app, "GET", "/me/security", Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "the api answers now");
    assert_eq!(security["totp"], true);
    let (_, options) = call(&app, "GET", "/auth/options", None, None).await;
    assert_eq!(options["setupNeeded"], false);
    assert_eq!(options["registration"], "closed");

    for (method, path) in [("GET", "/setup"), ("POST", "/setup/start")] {
        let body = (method == "POST").then(|| json!({ "token": link(&state) }));
        let (status, answer) = call(&app, method, path, None, body).await;
        assert_eq!(
            (status, answer["error"].as_str()),
            (StatusCode::GONE, Some("setupDone")),
            "{path}"
        );
    }
    let (status, _) = login(&app, "root", PASSWORD).await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "the password alone is not enough"
    );
    let log = events(&state).await;
    assert!(log.iter().any(|e| e == "setup_done"), "{log:?}");

    // a restart on the same database stays locked
    let again = AppState::for_tests(state.config.as_ref().clone(), state.db.clone());
    let app = router(again.clone());
    let (status, _) = start(&app, &link(&again)).await;
    assert_eq!(status, StatusCode::GONE);
    let (status, _) = call(&app, "GET", "/me", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn wrong_codes_end_the_session() {
    let (state, app) = fresh(false, "");
    let session = session(&state, &app).await;
    totp_secret(&app, &session).await;
    for _ in 0..5 {
        let (status, _) =
            finish(&app, &session, &json!({ "type": "totp", "code": "000000" })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, body) = call(
        &app,
        "POST",
        "/setup/totp",
        None,
        Some(json!({ "session": session, "username": "root" })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("setupLinkInvalid"))
    );
}

#[tokio::test]
async fn two_finishes_at_once_make_one_admin() {
    let (state, app) = fresh(false, "");
    let session = session(&state, &app).await;
    let secret = totp_secret(&app, &session).await;
    let factor = json!({ "type": "totp", "code": totp::code_now(&secret, state.clock.now()) });
    let (a, b) = tokio::join!(
        finish(&app, &session, &factor),
        finish(&app, &session, &factor)
    );
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::CREATED, StatusCode::GONE]);
    let admins: i64 = state
        .db
        .call(|c| Ok(c.query_row("SELECT count(*) FROM users", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(admins, 1);
}

#[tokio::test]
async fn a_passkey_admin_signs_in_without_a_username() {
    let (state, app) = fresh(false, ORIGIN);
    let (_, status) = call(&app, "GET", "/setup", None, None).await;
    assert_eq!(status["passkeys"], true);
    let session = session(&state, &app).await;
    let (status, options) = call(
        &app,
        "POST",
        "/setup/passkey",
        None,
        Some(json!({ "session": session, "username": "root", "displayName": "Root" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(
        options["publicKey"]["authenticatorSelection"]["residentKey"],
        "required"
    );
    let origin = Url::parse(ORIGIN).unwrap();
    let mut key = SoftPasskey::new(true);
    let credential = key
        .do_registration(origin, serde_json::from_value(options).unwrap())
        .unwrap();
    let credential = serde_json::to_value(credential).unwrap();
    let (status, done) = finish(
        &app,
        &session,
        &json!({ "type": "passkey", "credential": credential, "name": "Laptop" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{done}");
    let user_id = done["session"]["user"]["id"].as_str().unwrap();
    let (status, session) =
        super::passkey_tests::discoverable_sign_in(&app, &mut key, &credential, user_id).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["user"]["username"], "root");
}

#[tokio::test]
async fn the_checks_say_what_the_server_sees_behind_the_proxy() {
    let (state, app) = fresh(true, ORIGIN);
    let session = {
        let body = json!({ "token": link(&state) });
        let mut req = Request::post("/api/v1/setup/start")
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::new("::1".parse().unwrap(), 1)));
        let response = app.clone().oneshot(req).await.unwrap();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        json["session"].as_str().unwrap().to_owned()
    };
    let checks = |headers: Vec<(&'static str, &'static str)>, path: &'static str| {
        let app = app.clone();
        let session = session.clone();
        async move {
            let mut req = Request::post(path)
                .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
                .header("content-type", "application/json")
                .header("x-setup-session", session.clone());
            for (name, value) in headers {
                req = req.header(name, value);
            }
            let body = if path.ends_with("upload") {
                Body::from(vec![0u8; 1024])
            } else {
                Body::from(json!({ "session": session }).to_string())
            };
            let mut req = req.body(body).unwrap();
            req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
                "127.0.0.1".parse().unwrap(),
                1,
            )));
            let response = app.oneshot(req).await.unwrap();
            let status = response.status();
            let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }
    };
    let proxied = vec![
        ("host", "127.0.0.1:8080"),
        ("x-forwarded-host", "notes.example.org"),
        ("x-forwarded-proto", "https"),
        ("x-forwarded-for", "198.51.100.4"),
    ];
    let (status, seen) = checks(proxied.clone(), "/api/v1/setup/checks").await;
    assert_eq!(status, StatusCode::OK, "{seen}");
    assert_eq!(seen["secure"], true);
    assert_eq!(seen["viaProxy"], true);
    assert_eq!(seen["forwardedFor"], true);
    assert_eq!(seen["forwardedProto"], "https");
    assert_eq!(seen["clientIp"], "198.51.100.4");
    assert_eq!(seen["host"], "notes.example.org");
    assert_eq!(seen["hostMatches"], true);
    assert_eq!(seen["partBytes"], 8 * 1024 * 1024);
    assert_eq!(seen["mail"], false);
    assert!((seen["serverTime"].as_i64().unwrap() - state.clock.now()).abs() < 5000);

    let (_, seen) = checks(
        vec![
            ("host", "notes.example.org"),
            ("x-forwarded-proto", "https"),
        ],
        "/api/v1/setup/checks",
    )
    .await;
    assert_eq!(seen["forwardedFor"], false, "a proxy that does not say who");
    let (_, seen) = checks(
        vec![
            ("host", "other.example.org"),
            ("x-forwarded-proto", "https"),
        ],
        "/api/v1/setup/checks",
    )
    .await;
    assert_eq!(seen["hostMatches"], false);

    let (status, got) = checks(proxied.clone(), "/api/v1/setup/upload").await;
    assert_eq!((status, &got["received"]), (StatusCode::OK, &json!(1024)));
}

#[tokio::test]
async fn a_command_line_admin_adds_a_factor_before_acting() {
    let (state, app) = fresh(false, "");
    let config = state.config.as_ref().clone();
    let first = crate::cli::create_admin(
        &state.db,
        &config,
        "root",
        None,
        PASSWORD,
        crate::cli::Origin::Command,
    )
    .await
    .unwrap();
    assert!(first);
    let (status, _) = start(&app, &link(&state)).await;
    assert_eq!(status, StatusCode::GONE, "setup is locked");
    let (status, session) = login(&app, "root", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["user"]["factorRequired"], true);
    let token = session["accessToken"].as_str().unwrap();
    for path in ["/admin/overview", "/admin/audit"] {
        let (status, body) = call(&app, "GET", path, Some(token), None).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (StatusCode::FORBIDDEN, Some("secondFactorRequired")),
            "{path}"
        );
    }
    let (_, setup) = call(&app, "POST", "/me/totp/setup", Some(token), None).await;
    let secret = setup["secret"].as_str().unwrap();
    let code = totp::code_now(secret, state.clock.now());
    let (status, _) = call(
        &app,
        "POST",
        "/me/totp/confirm",
        Some(token),
        Some(json!({ "code": code, "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, me) = call(&app, "GET", "/me", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(me.get("factorRequired").is_none(), "{me}");
    let (status, _) = call(&app, "GET", "/admin/overview", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);

    let password = crate::cli::reset_admin(&state.db, &config, "root")
        .await
        .unwrap();
    let (status, _) = call(&app, "GET", "/me", Some(token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "every device signed out");
    let (status, session) = login(&app, "root", &password).await;
    assert_eq!(status, StatusCode::OK, "no factor left to ask for");
    let token = session["accessToken"].as_str().unwrap();
    let (status, _) = call(&app, "GET", "/admin/overview", Some(token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a factor is due again");
    let log = events(&state).await;
    assert!(log.iter().any(|e| e == "admin_created"));
    assert!(log.iter().any(|e| e == "admin_reset"));
}

#[tokio::test]
async fn an_admin_without_a_second_step_when_chosen() {
    let (state, app) = fresh(false, "");
    let session = session(&state, &app).await;
    let (status, done) = finish(&app, &session, &json!({ "type": "none" })).await;
    assert_eq!(status, StatusCode::CREATED, "{done}");
    assert_eq!(
        done["recoveryCodes"].as_array().unwrap().len(),
        0,
        "nothing to stand in for"
    );
    let token = done["session"]["accessToken"].as_str().unwrap().to_owned();

    // the password alone signs in, and the admin acts as admin
    let (status, signed) = login(&app, "root", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{signed}");
    let (status, overview) = call(&app, "GET", "/admin/overview", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    assert_eq!(overview["adminsNeedSecondStep"], false);

    // turned back on: an admin without a second step waits for one
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/second-step",
        Some(&token),
        Some(json!({ "required": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(&app, "GET", "/admin/overview", Some(&token), None).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("secondFactorRequired"))
    );
    let log = events(&state).await;
    assert!(
        log.iter().any(|e| e == "admin_second_step_changed"),
        "{log:?}"
    );
}

/// The browser's part of an end-to-end encrypted admin: the OPAQUE
/// registration through setup and the account keys. The `e2ee` field of
/// the finish request, and the keys as the browser keeps them.
async fn e2ee_made(app: &Router, session: &str) -> (Value, app_e2ee::NewAccount) {
    let start = app_e2ee::registration_start(PASSWORD).unwrap();
    let (status, answer) = call(
        app,
        "POST",
        "/setup/registration",
        None,
        Some(json!({ "session": session, "username": "Root", "request": start.request })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let finish = app_e2ee::registration_finish(
        &start.state,
        PASSWORD,
        answer["response"].as_str().unwrap(),
        "Root",
        app_e2ee::Ksf::CURRENT,
    )
    .unwrap();
    let account = app_e2ee::new_account(&finish.export_key);
    let e2ee = json!({
        "upload": finish.upload,
        "ksf": app_e2ee::Ksf::CURRENT.number(),
        "keys": account.bundle,
        "recoveryVerifier": account.recovery_verifier,
    });
    (e2ee, account)
}

/// Finishes setup as an end-to-end encrypted admin: no password sent.
async fn finish_e2ee(
    app: &Router,
    session: &str,
    factor: &Value,
    e2ee: &Value,
) -> (StatusCode, Value) {
    let mut body = finish_body(session, factor);
    body.as_object_mut().unwrap().remove("password");
    body["e2ee"] = e2ee.clone();
    call(app, "POST", "/setup/finish", None, Some(body)).await
}

/// Signs in over OPAQUE as an app does: the finish answer and the export
/// key.
async fn opaque_sign_in(app: &Router) -> (StatusCode, Value, Vec<u8>) {
    let start = app_e2ee::login_start(PASSWORD).unwrap();
    let (status, started) = call(
        app,
        "POST",
        "/auth/opaque/login/start",
        None,
        Some(json!({ "username": "root", "request": start.request })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let done = app_e2ee::login_finish(
        &start.state,
        PASSWORD,
        started["response"].as_str().unwrap(),
        "root",
        super::tests::ksf(&started),
    )
    .unwrap();
    let (status, answer) = call(
        app,
        "POST",
        "/auth/opaque/login/finish",
        None,
        Some(json!({ "challenge": started["challenge"], "finalization": done.finalization, "device": device("phone") })),
    )
    .await;
    (status, answer, done.export_key.to_vec())
}

/// An OPAQUE sign-in with its authenticator code, a fresh one each time:
/// the session and the export key.
async fn e2ee_sign_in(state: &AppState, app: &Router, secret: &str) -> (Value, Vec<u8>) {
    state.clock.advance(MINUTE);
    let (status, needed, export_key) = opaque_sign_in(app).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{needed}");
    let code = totp::code_now(secret, state.clock.now());
    let (status, session) = call(
        app,
        "POST",
        "/auth/second-factor",
        None,
        Some(json!({ "challenge": needed["challenge"], "method": "totp", "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    (session, export_key)
}

#[tokio::test(flavor = "multi_thread")]
async fn an_end_to_end_encrypted_admin_signs_in_and_syncs_sealed_notes() {
    let (state, app) = fresh(false, "");
    let session = session(&state, &app).await;
    let secret = totp_secret(&app, &session).await;
    let (e2ee, account) = e2ee_made(&app, &session).await;
    let factor = json!({ "type": "totp", "code": totp::code_now(&secret, state.clock.now()) });

    let mut leaky = finish_body(&session, &factor);
    leaky["e2ee"] = e2ee.clone();
    let (status, body) = call(&app, "POST", "/setup/finish", None, Some(leaky)).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid")),
        "the password never comes with it"
    );

    let (status, done) = finish_e2ee(&app, &session, &factor, &e2ee).await;
    assert_eq!(status, StatusCode::CREATED, "{done}");
    assert_eq!(done["session"]["user"]["kind"], "e2ee");
    assert_eq!(done["session"]["user"]["username"], "root");
    assert_eq!(done["session"]["user"]["role"], "admin");
    assert_eq!(done["recoveryCodes"].as_array().unwrap().len(), 10);
    let (hash, record): (String, Vec<u8>) = state
        .db
        .call(|c| {
            Ok(c.query_row(
                "SELECT password_hash, opaque_record FROM users WHERE username = 'root'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .await
        .unwrap();
    assert!(hash.is_empty());
    assert!(!String::from_utf8_lossy(&record).contains(PASSWORD));
    let log = events(&state).await;
    assert!(log.iter().any(|e| e == "setup_done"), "{log:?}");

    // the password form never opens it, OPAQUE and the second step do
    let (status, _) = login(&app, "root", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (phone_session, export_key) = e2ee_sign_in(&state, &app, &secret).await;
    let token = phone_session["accessToken"].as_str().unwrap();
    let (_, keys) = call(&app, "GET", "/me/keys", Some(token), None).await;
    let bundle: app_e2ee::KeyBundle = serde_json::from_value(keys["keys"].clone()).unwrap();
    assert_eq!(bundle, account.bundle);
    let master = app_e2ee::AccountKeys::open_with_password(&bundle, &export_key).unwrap();
    assert_eq!(master.as_bytes(), account.master.as_bytes());

    // the recovery code shown once in the browser opens the account
    let verifier = app_e2ee::AccountKeys::recovery_verifier(&account.recovery_code).unwrap();
    let (status, recovery) = call(
        &app,
        "POST",
        "/auth/recovery/start",
        None,
        Some(json!({ "username": "root", "verifier": verifier })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recovery}");

    // a note sealed on the phone reaches the laptop, the server holds only
    // what it cannot read
    let phone = super::sync_tests::linked(&app, &phone_session, Some(&master));
    phone
        .0
        .lock()
        .unwrap()
        .create_note(&app_core::store::NewNote {
            title: "Garden".into(),
            body: "Order more cedar posts".into(),
            ..app_core::store::NewNote::default()
        })
        .unwrap();
    let (_, pushed) = super::sync_tests::round(phone).await;
    assert!(pushed.pushed > 0, "{pushed:?}");
    let (_, page) = call(&app, "GET", "/sync/changes", Some(token), None).await;
    let changes = page["changes"].as_array().unwrap();
    assert!(!changes.is_empty(), "{page}");
    for change in changes {
        let data = base64::engine::general_purpose::STANDARD
            .decode(change["data"].as_str().unwrap())
            .unwrap();
        let text = String::from_utf8_lossy(&data);
        assert!(
            !text.contains("cedar") && !text.contains("Garden"),
            "{text}"
        );
    }

    let (laptop_session, export_key) = e2ee_sign_in(&state, &app, &secret).await;
    let opened = app_e2ee::AccountKeys::open_with_password(&bundle, &export_key).unwrap();
    let laptop = super::sync_tests::linked(&app, &laptop_session, Some(&opened));
    let (laptop, came) = super::sync_tests::round(laptop).await;
    assert!(
        came.taken.applied > 0 && came.taken.rejected == 0,
        "{came:?}"
    );
    assert!(super::sync_tests::view_of(&laptop).contains("Order more cedar posts"));
}

#[tokio::test]
async fn an_end_to_end_encrypted_admin_with_a_passkey_signs_in_to_the_admin_page() {
    let (state, app) = fresh(false, ORIGIN);
    let session = session(&state, &app).await;
    let (status, options) = call(
        &app,
        "POST",
        "/setup/passkey",
        None,
        Some(json!({ "session": session, "username": "root" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    let mut key = SoftPasskey::new(true);
    let credential = key
        .do_registration(
            Url::parse(ORIGIN).unwrap(),
            serde_json::from_value(options).unwrap(),
        )
        .unwrap();
    let credential = serde_json::to_value(credential).unwrap();
    let (e2ee, _) = e2ee_made(&app, &session).await;
    let factor = json!({ "type": "passkey", "credential": credential, "name": "Laptop" });
    let (status, done) = finish_e2ee(&app, &session, &factor, &e2ee).await;
    assert_eq!(status, StatusCode::CREATED, "{done}");
    let user_id = done["session"]["user"]["id"].as_str().unwrap();

    // the admin page: the passkey alone
    let (status, signed) =
        super::passkey_tests::discoverable_sign_in(&app, &mut key, &credential, user_id).await;
    assert_eq!(status, StatusCode::OK, "{signed}");
    assert_eq!(signed["user"]["kind"], "e2ee");
    let token = signed["accessToken"].as_str().unwrap();
    let (status, overview) = call(&app, "GET", "/admin/overview", Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{overview}");

    // an app: the password, then the passkey as its second step
    let (status, needed, _) = opaque_sign_in(&app).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{needed}");
    assert!(
        needed["methods"]
            .as_array()
            .unwrap()
            .contains(&json!("passkey")),
        "{needed}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_standard_admin_from_setup_signs_in_and_syncs() {
    let (state, app) = fresh(false, "");
    let session = session(&state, &app).await;
    let secret = totp_secret(&app, &session).await;
    let factor = json!({ "type": "totp", "code": totp::code_now(&secret, state.clock.now()) });
    let (status, done) = finish(&app, &session, &factor).await;
    assert_eq!(status, StatusCode::CREATED, "{done}");
    assert_eq!(done["session"]["user"]["kind"], "standard");

    state.clock.advance(MINUTE);
    let (status, needed) = login(&app, "root", PASSWORD).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{needed}");
    let code = totp::code_now(&secret, state.clock.now());
    let (status, signed) = call(
        &app,
        "POST",
        "/auth/second-factor",
        None,
        Some(json!({ "challenge": needed["challenge"], "method": "totp", "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{signed}");
    let (_, keys) = call(
        &app,
        "GET",
        "/me/keys",
        Some(signed["accessToken"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(keys["error"], "invalid", "no keys of its own: {keys}");

    let phone = super::sync_tests::linked(&app, &signed, None);
    phone
        .0
        .lock()
        .unwrap()
        .create_note(&app_core::store::NewNote {
            title: "Garden".into(),
            ..app_core::store::NewNote::default()
        })
        .unwrap();
    let (_, pushed) = super::sync_tests::round(phone).await;
    assert!(pushed.pushed > 0, "{pushed:?}");
}

#[tokio::test]
async fn setup_is_locked_after_an_end_to_end_encrypted_admin() {
    let (state, app) = fresh(false, "");
    let session = session(&state, &app).await;
    let (e2ee, _) = e2ee_made(&app, &session).await;
    let (status, done) = finish_e2ee(&app, &session, &json!({ "type": "none" }), &e2ee).await;
    assert_eq!(status, StatusCode::CREATED, "{done}");

    let (status, _) = start(&app, &link(&state)).await;
    assert_eq!(status, StatusCode::GONE, "a new link");
    let start = app_e2ee::registration_start(PASSWORD).unwrap();
    let (status, _) = call(
        &app,
        "POST",
        "/setup/registration",
        None,
        Some(json!({ "session": session, "username": "other", "request": start.request })),
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "another registration");
    for body in [finish_body(&session, &json!({ "type": "none" })), {
        let mut again = finish_body(&session, &json!({ "type": "none" }));
        again.as_object_mut().unwrap().remove("password");
        again["e2ee"] = e2ee.clone();
        again
    }] {
        let (status, body) = call(&app, "POST", "/setup/finish", None, Some(body)).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (StatusCode::GONE, Some("setupDone"))
        );
    }
    let users: i64 = state
        .db
        .call(|c| Ok(c.query_row("SELECT count(*) FROM users", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(users, 1);
}
