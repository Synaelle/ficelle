// SPDX-License-Identifier: AGPL-3.0-or-later

//! The account endpoints through the full router, as a client sees them.

use app_protocol::{PROTOCOL_HEADER, PROTOCOL_VERSION};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::accounts::store;
use crate::clock::MINUTE;
use crate::config::{AuthConfig, Config};
use crate::db::Db;
use crate::routes::{AppState, router};

/// A fresh server with cheap password hashing, past setup. The setup flow
/// itself is tested in `setup_tests`.
pub(super) async fn server() -> (AppState, Router) {
    server_with(Config::default()).await
}

/// [`server`] from a config of the test's own.
pub(super) async fn server_with(config: Config) -> (AppState, Router) {
    let config = Config {
        auth: AuthConfig {
            argon2_memory_kib: 64,
            argon2_iterations: 1,
            ..config.auth
        },
        ..config
    };
    let state = AppState::for_tests(config, Db::in_memory().unwrap());
    state.db.call(|c| crate::setup::lock(c)).await.unwrap();
    let app = router(state.clone());
    (state, app)
}

pub(super) async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(format!("/api/v1{path}"))
        .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string());
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req
            .header("content-type", "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The stretching an OPAQUE start answer says the record uses.
pub(super) fn ksf(started: &Value) -> app_e2ee::Ksf {
    let number = u8::try_from(started["ksf"].as_u64().unwrap()).unwrap();
    app_e2ee::Ksf::from_number(number).unwrap()
}

pub(super) fn device(name: &str) -> Value {
    json!({ "name": name, "platform": "android" })
}

/// Server with the first admin, returns the admin's access token.
pub(super) async fn with_admin() -> (AppState, Router, String) {
    admin_on(server().await).await
}

/// The first admin on a given server, made directly with a password alone,
/// so the tests sign in as it without a second factor.
pub(super) async fn admin_on((state, app): (AppState, Router)) -> (AppState, Router, String) {
    let hash = super::auth::hash_password(&state, "admin password".into())
        .await
        .unwrap();
    let auth = state.config.auth.clone();
    let device = crate::accounts::device_info(&serde_json::from_value(device("laptop")).unwrap());
    let now = state.clock.now();
    let tokens = state
        .db
        .call(move |c| {
            let user = store::insert_user(
                c,
                "admin",
                "admin",
                &hash,
                app_protocol::account::Role::Admin,
                now,
            )?
            .unwrap();
            let tokens = store::create_session(c, &user.id, &device, now, &auth)?;
            // as setup logs it
            store::audit(
                c,
                now,
                &store::Event {
                    name: "setup_done",
                    user_id: Some(&user.id),
                    ..store::Event::default()
                },
            )?;
            Ok(tokens)
        })
        .await
        .unwrap();
    (state, app, tokens.access)
}

/// Another standard account, as a server that had several still holds,
/// and its sign-in answer. Its password is "<name> password".
pub(super) async fn person(state: &AppState, name: &str) -> Value {
    let hash = super::auth::hash_password(state, format!("{name} password"))
        .await
        .unwrap();
    let auth = state.config.auth.clone();
    let device = crate::accounts::device_info(&serde_json::from_value(device("phone")).unwrap());
    let now = state.clock.now();
    let name = name.to_owned();
    let (user, tokens) = state
        .db
        .call(move |c| {
            let user = store::insert_user(
                c,
                &name,
                &name,
                &hash,
                app_protocol::account::Role::User,
                now,
            )?
            .unwrap();
            let tokens = store::create_session(c, &user.id, &device, now, &auth)?;
            Ok((user, tokens))
        })
        .await
        .unwrap();
    serde_json::to_value(super::auth::session(&user, tokens)).unwrap()
}

/// An e2ee account, as sign-up made them before servers were for one
/// person: its sign-in answer and the keys it was made with.
pub(super) async fn e2ee_person(state: &AppState, name: &str) -> (Value, E2eeMade) {
    let password = format!("{name} password");
    let start = app_e2ee::registration_start(&password).unwrap();
    let setup = super::e2ee::setup(state).await.unwrap();
    let response = setup.registration_response(&start.request, name).unwrap();
    let finish = app_e2ee::registration_finish(
        &start.state,
        &password,
        &response,
        name,
        app_e2ee::Ksf::CURRENT,
    )
    .unwrap();
    let made = app_e2ee::new_account(&finish.export_key);
    let session = e2ee_account(
        state,
        name,
        &finish.upload,
        app_e2ee::Ksf::CURRENT.number(),
        &made,
    )
    .await;
    (
        session,
        E2eeMade {
            export_key: finish.export_key,
            bundle: made.bundle,
        },
    )
}

/// Stores an e2ee account from its OPAQUE upload and new keys, as sign-up
/// did, and signs a device in: its sign-in answer.
pub(super) async fn e2ee_account(
    state: &AppState,
    name: &str,
    upload: &str,
    ksf: u8,
    made: &app_e2ee::NewAccount,
) -> Value {
    let record = app_e2ee::server::Setup::record(upload).unwrap();
    let bundle: app_protocol::e2ee::KeyBundle =
        serde_json::from_value(serde_json::to_value(&made.bundle).unwrap()).unwrap();
    let verifier = made.recovery_verifier.clone();
    let auth = state.config.auth.clone();
    let device = crate::accounts::device_info(&serde_json::from_value(device("phone")).unwrap());
    let now = state.clock.now();
    let n = name.trim().to_ascii_lowercase();
    let (user, tokens) = state
        .db
        .call(move |c| {
            let user =
                store::insert_user(c, &n, &n, "", app_protocol::account::Role::User, now)?.unwrap();
            crate::accounts::e2ee::make_e2ee(c, &user.id, &record, ksf, &bundle, &verifier)?;
            let user = store::user_by_id(c, &user.id)?.unwrap();
            let tokens = store::create_session(c, &user.id, &device, now, &auth)?;
            Ok((user, tokens))
        })
        .await
        .unwrap();
    serde_json::to_value(super::auth::session(&user, tokens)).unwrap()
}

/// A device of the app core signed in through the router.
pub(super) type Device = (
    std::sync::Mutex<app_core::store::Store>,
    super::sync_tests::Wire,
    tempfile::TempDir,
);

/// An e2ee account and a linked device holding its master key.
pub(super) async fn e2ee_device(state: &AppState, app: &Router, name: &str) -> (Value, Device) {
    let (session, made) = e2ee_person(state, name).await;
    let master = app_e2ee::AccountKeys::open_with_password(&made.bundle, &made.export_key).unwrap();
    let device = super::sync_tests::linked(app, &session, Some(&master));
    (session, device)
}

/// What an e2ee account was made with, for the tests that open its keys.
pub(super) struct E2eeMade {
    pub(super) export_key: zeroize::Zeroizing<Vec<u8>>,
    pub(super) bundle: app_e2ee::KeyBundle,
}

pub(super) fn token(session: &Value) -> &str {
    session["accessToken"].as_str().unwrap()
}

pub(super) fn user_id(session: &Value) -> &str {
    session["user"]["id"].as_str().unwrap()
}

pub(super) async fn login(app: &Router, username: &str, password: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/auth/login",
        None,
        Some(json!({ "username": username, "password": password, "device": device("phone") })),
    )
    .await
}

pub(super) async fn events(state: &AppState) -> Vec<String> {
    state
        .db
        .call(|c| {
            let mut stmt = c.prepare("SELECT event FROM audit_log ORDER BY id")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn sign_in_me_and_wrong_passwords_lock_the_account_for_a_while() {
    let (state, app, _) = with_admin().await;
    let (status, session) = login(&app, "ADMIN", "admin password").await;
    assert_eq!(status, StatusCode::OK);
    let (status, me) = call(&app, "GET", "/me", session["accessToken"].as_str(), None).await;
    assert_eq!(
        (status, me["username"].as_str()),
        (StatusCode::OK, Some("admin"))
    );
    let (status, _) = call(&app, "GET", "/me", Some("at_not_a_token"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = login(&app, "nobody", "whatever password").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("badCredentials")),
        "unknown user, same answer"
    );
    for _ in 0..5 {
        let (status, _) = login(&app, "admin", "wrong password").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, body) = login(&app, "admin", "admin password").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::TOO_MANY_REQUESTS, Some("lockedOut")),
        "even the right password waits"
    );
    assert!(body["retryAfterSeconds"].as_u64().unwrap() <= 60);

    state.clock.advance(MINUTE + 1000);
    let (status, _) = login(&app, "admin", "admin password").await;
    assert_eq!(status, StatusCode::OK);
    let log = events(&state).await;
    assert!(
        log.contains(&"locked_out".to_string()) && log.contains(&"sign_in".to_string()),
        "{log:?}"
    );
}

#[tokio::test]
async fn a_refresh_token_used_twice_closes_the_device() {
    let (state, app, _) = with_admin().await;
    let (_, first) = login(&app, "admin", "admin password").await;
    let old = first["refreshToken"].clone();
    let (status, second) = call(
        &app,
        "POST",
        "/auth/refresh",
        None,
        Some(json!({ "refreshToken": old })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        second["deviceId"], first["deviceId"],
        "same device, new tokens"
    );
    // a copy comes back later than a retry after a lost answer would
    state
        .clock
        .advance(crate::accounts::store::REFRESH_RETRY_MS + 1000);

    let (status, body) = call(
        &app,
        "POST",
        "/auth/refresh",
        None,
        Some(json!({ "refreshToken": old })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("sessionRevoked"))
    );
    let (status, _) = call(&app, "GET", "/me", second["accessToken"].as_str(), None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the thief's copy and the owner's both stop"
    );
    assert!(events(&state).await.contains(&"token_reuse".to_string()));
}

#[tokio::test]
async fn changing_the_password_signs_out_the_other_devices() {
    let (_, app, laptop) = with_admin().await;
    let (_, phone) = login(&app, "admin", "admin password").await;
    let phone = phone["accessToken"].as_str().unwrap().to_owned();
    let (_, devices) = call(&app, "GET", "/me/devices", Some(&laptop), None).await;
    let devices = devices.as_array().unwrap().clone();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices.iter().filter(|d| d["current"] == true).count(), 1);

    let (status, body) = call(
        &app,
        "POST",
        "/me/password",
        Some(&laptop),
        Some(json!({ "current": "wrong", "new": "new password" })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("badCredentials"))
    );
    let (status, _) = call(
        &app,
        "POST",
        "/me/password",
        Some(&laptop),
        Some(json!({ "current": "admin password", "new": "new password" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(&app, "GET", "/me", Some(&phone), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "GET", "/me", Some(&laptop), None).await.0,
        StatusCode::OK
    );
    assert_eq!(login(&app, "admin", "new password").await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_device_can_be_signed_out_from_another() {
    let (_, app, laptop) = with_admin().await;
    let (_, phone) = login(&app, "admin", "admin password").await;
    let phone_id = phone["deviceId"].as_str().unwrap();
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/me/devices/{phone_id}"),
        Some(&laptop),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(&app, "GET", "/me", phone["accessToken"].as_str(), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, _) = call(
        &app,
        "POST",
        "/auth/refresh",
        None,
        Some(json!({ "refreshToken": phone["refreshToken"] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sign_in_attempts_are_rate_limited_per_address() {
    let (_, app) = server().await;
    // a new name each time: one name alone locks after five, as an account does
    for n in 0..20 {
        let (status, _) = login(&app, &format!("nobody{n}"), "whatever password").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, body) = login(&app, "nobody20", "whatever password").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::TOO_MANY_REQUESTS, Some("rateLimited"))
    );
    assert!(body["retryAfterSeconds"].as_u64().unwrap() >= 1);
}

/// Apps and pages from before servers were for one person may still ask
/// for what left: they get the usual not found.
#[tokio::test]
async fn what_left_with_servers_for_several_people_is_not_found() {
    let (_, app, admin) = with_admin().await;
    for (method, path) in [
        ("POST", "/auth/register"),
        ("POST", "/auth/e2ee/register"),
        ("GET", "/invites"),
        ("POST", "/invites"),
        ("GET", "/me/groups"),
        ("POST", "/me/deletion"),
        ("GET", "/admin/users"),
        ("GET", "/admin/users/x"),
        ("GET", "/admin/registration"),
        ("PUT", "/admin/account-kinds"),
        ("GET", "/admin/groups"),
        ("GET", "/admin/one-person"),
        ("GET", "/admin/feedback/access"),
        ("PUT", "/admin/feedback/readers/x"),
        ("POST", "/admin/backups/restore-person"),
        ("GET", "/sync/spaces"),
        ("POST", "/sync/spaces"),
        ("GET", "/sync/people?name=x"),
        ("GET", "/sync/key-pair"),
        ("POST", "/auth/sso/start"),
    ] {
        let body = (method != "GET").then(|| json!({}));
        let (status, answer) = call(&app, method, path, Some(&admin), body).await;
        assert_eq!(
            (status, answer["error"].as_str()),
            (StatusCode::NOT_FOUND, Some("notFound")),
            "{method} {path}"
        );
    }
    let (status, options) = call(&app, "GET", "/auth/options", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(options["registration"], "closed");
    assert_eq!(
        options["accountKinds"], "standard",
        "the kind of the one account"
    );
}
