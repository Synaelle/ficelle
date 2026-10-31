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
    let state = AppState::new(config, Db::in_memory().unwrap());
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
async fn registration_follows_the_mode_and_invites_work_once() {
    let (_, app, admin) = with_admin().await;
    let anna =
        json!({ "username": "anna", "password": "anna password", "device": device("phone") });
    let (status, body) = call(&app, "POST", "/auth/register", None, Some(anna.clone())).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("registrationClosed"))
    );

    let (status, invite) = call(&app, "POST", "/invites", Some(&admin), Some(json!({}))).await;
    assert_eq!(status, StatusCode::CREATED);
    let token = invite["token"].as_str().unwrap().to_owned();
    let mut with_invite = anna.clone();
    with_invite["invite"] = json!(token);
    let (status, session) = call(
        &app,
        "POST",
        "/auth/register",
        None,
        Some(with_invite.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{session}");
    let anna_token = session["accessToken"].as_str().unwrap().to_owned();

    with_invite["username"] = json!("bob");
    let (status, body) = call(&app, "POST", "/auth/register", None, Some(with_invite)).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("inviteInvalid"))
    );

    // a regular user invites only in invite mode
    let (status, _) = call(&app, "POST", "/invites", Some(&anna_token), Some(json!({}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/registration",
        Some(&anna_token),
        Some(json!({ "registration": "open" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "admins only");
    call(
        &app,
        "PUT",
        "/admin/registration",
        Some(&admin),
        Some(json!({ "registration": "invite" })),
    )
    .await;
    let (status, _) = call(
        &app,
        "POST",
        "/invites",
        Some(&anna_token),
        Some(json!({ "days": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, mine) = call(&app, "GET", "/invites", Some(&anna_token), None).await;
    assert_eq!(
        mine.as_array().unwrap().len(),
        1,
        "a user sees only their invites"
    );

    call(
        &app,
        "PUT",
        "/admin/registration",
        Some(&admin),
        Some(json!({ "registration": "open" })),
    )
    .await;
    let carl =
        json!({ "username": "carl", "password": "carl password", "device": device("phone") });
    let (status, _) = call(&app, "POST", "/auth/register", None, Some(carl.clone())).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = call(&app, "POST", "/auth/register", None, Some(carl)).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::CONFLICT, Some("usernameTaken"))
    );
    let weak = json!({ "username": "dana", "password": "short", "device": device("phone") });
    let (status, body) = call(&app, "POST", "/auth/register", None, Some(weak)).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("passwordWeak"))
    );
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
    for _ in 0..20 {
        let (status, _) = login(&app, "nobody", "whatever password").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, body) = login(&app, "nobody", "whatever password").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::TOO_MANY_REQUESTS, Some("rateLimited"))
    );
    assert!(body["retryAfterSeconds"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn admins_add_people_who_then_sign_in() {
    let (_, app, admin) = with_admin().await;
    let (status, user) = call(
        &app,
        "POST",
        "/admin/users",
        Some(&admin),
        Some(json!({ "username": "Erin", "password": "erin password", "displayName": "Erin M." })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        (user["username"].as_str(), user["role"].as_str()),
        (Some("erin"), Some("user"))
    );
    assert_eq!(login(&app, "erin", "erin password").await.0, StatusCode::OK);
    let (status, body) = call(
        &app,
        "POST",
        "/admin/users",
        Some(&admin),
        Some(json!({ "username": "bad name!", "password": "erin password" })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("usernameInvalid"))
    );
    let (status, body) = call(
        &app,
        "POST",
        "/admin/users",
        Some(&admin),
        Some(json!({ "username": 3 })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid")),
        "{body}"
    );
}

#[tokio::test]
async fn an_invite_can_last_an_hour() {
    let (state, app, admin) = with_admin().await;
    let now = state.clock.now();
    let (status, short) = call(
        &app,
        "POST",
        "/invites",
        Some(&admin),
        Some(json!({ "hours": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{short}");
    let life = |invite: &Value| invite["expiresAt"].as_i64().unwrap() - now;
    // the clock moves on a little between the two
    assert!((crate::clock::HOUR..crate::clock::HOUR + 5_000).contains(&life(&short)));
    let (_, long) = call(
        &app,
        "POST",
        "/invites",
        Some(&admin),
        Some(json!({ "days": 3 })),
    )
    .await;
    assert!((3 * crate::clock::DAY..3 * crate::clock::DAY + 5_000).contains(&life(&long)));
}
