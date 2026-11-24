// SPDX-License-Identifier: AGPL-3.0-or-later

//! End-to-end encrypted accounts through the full router, with the client
//! half from `app-e2ee` playing the app's part.

use app_e2ee::{
    AccountKeys, KeyBundle, login_finish, login_start, new_account, registration_finish,
    registration_start,
};
use axum::Router;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::tests::{call, device, events, login, with_admin};
use crate::accounts::totp;
use crate::clock::MINUTE;
use crate::routes::AppState;

const PASSWORD: &str = "correct horse battery";

/// What the client keeps after signing up.
struct Account {
    token: String,
    export_key: Vec<u8>,
    bundle: KeyBundle,
    recovery_code: String,
}

/// A server with its admin.
async fn open_server() -> (AppState, Router, String) {
    with_admin().await
}

/// The OPAQUE registration of a password, as the client runs it.
async fn register_password(app: &Router, username: &str, password: &str) -> (String, Vec<u8>) {
    register_password_with(app, username, password, app_e2ee::Ksf::CURRENT).await
}

/// The same, the password stretched by `ksf`.
async fn register_password_with(
    app: &Router,
    username: &str,
    password: &str,
    ksf: app_e2ee::Ksf,
) -> (String, Vec<u8>) {
    let start = registration_start(password).unwrap();
    let (status, answer) = call(
        app,
        "POST",
        "/auth/opaque/registration",
        None,
        Some(json!({ "username": username, "request": start.request })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let finish = registration_finish(
        &start.state,
        password,
        answer["response"].as_str().unwrap(),
        username,
        ksf,
    )
    .unwrap();
    (finish.upload, finish.export_key.to_vec())
}

/// An e2ee account as sign-up made them before servers were for one person.
async fn sign_up(
    state: &AppState,
    app: &Router,
    username: &str,
) -> (StatusCode, Value, Option<Account>) {
    let (upload, export_key) = register_password(app, username, PASSWORD).await;
    let account = new_account(&export_key);
    let body = super::tests::e2ee_account(state, username, &upload, 2, &account).await;
    let made = body["accessToken"].as_str().map(|token| Account {
        token: token.into(),
        export_key,
        bundle: account.bundle.clone(),
        recovery_code: account.recovery_code.to_string(),
    });
    (StatusCode::CREATED, body, made)
}

/// Signs in over OPAQUE: the finish answer and the export key, if the
/// client could finish at all.
async fn opaque_login(
    app: &Router,
    username: &str,
    password: &str,
) -> ((StatusCode, Value), Option<Vec<u8>>) {
    let start = login_start(password).unwrap();
    let (status, started) = call(
        app,
        "POST",
        "/auth/opaque/login/start",
        None,
        Some(json!({ "username": username, "request": start.request })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let finish = login_finish(
        &start.state,
        password,
        started["response"].as_str().unwrap(),
        username,
        super::tests::ksf(&started),
    );
    // a wrong password fails on the client: it still finishes on the
    // server with junk, as an attacker would, so the failure is counted
    let (finalization, export_key) = match finish {
        Ok(f) => (f.finalization, Some(f.export_key.to_vec())),
        Err(_) => (app_e2ee::encode(&[0u8; 64]), None),
    };
    let answer = call(
        app,
        "POST",
        "/auth/opaque/login/finish",
        None,
        Some(json!({ "challenge": started["challenge"], "finalization": finalization, "device": device("laptop") })),
    )
    .await;
    (answer, export_key)
}

/// A proof of the password, from `/me/confirm`.
async fn proof(app: &Router, token: &str, username: &str, password: &str) -> (StatusCode, Value) {
    let start = login_start(password).unwrap();
    let (_, started) = call(
        app,
        "POST",
        "/me/confirm/start",
        Some(token),
        Some(json!({ "request": start.request })),
    )
    .await;
    let finalization = login_finish(
        &start.state,
        password,
        started["response"].as_str().unwrap(),
        username,
        super::tests::ksf(&started),
    )
    .map_or_else(|_| app_e2ee::encode(&[0u8; 64]), |f| f.finalization);
    call(
        app,
        "POST",
        "/me/confirm/finish",
        Some(token),
        Some(json!({ "challenge": started["challenge"], "finalization": finalization })),
    )
    .await
}

#[tokio::test]
async fn sign_up_and_sign_in_without_the_password_reaching_the_server() {
    let (state, app, _) = open_server().await;
    let (status, session, account) = sign_up(&state, &app, "Zoe").await;
    assert_eq!(status, StatusCode::CREATED, "{session}");
    assert_eq!(session["user"]["kind"], "e2ee");
    assert_eq!(session["user"]["username"], "zoe");
    let account = account.unwrap();

    // the server hands the keys back, only the password opens them
    let (_, keys) = call(&app, "GET", "/me/keys", Some(&account.token), None).await;
    let bundle: KeyBundle = serde_json::from_value(keys["keys"].clone()).unwrap();
    assert_eq!(bundle, account.bundle);

    let ((status, signed), export_key) = opaque_login(&app, "ZOE", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{signed}");
    let export_key = export_key.unwrap();
    assert_eq!(export_key, account.export_key);
    AccountKeys::open_with_password(&bundle, &export_key).unwrap();

    // the password form never opens an e2ee account
    let (status, _) = login(&app, "zoe", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let stored: Vec<u8> = state
        .db
        .call(|c| {
            Ok(c.query_row(
                "SELECT opaque_record FROM users WHERE username = 'zoe'",
                [],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&stored);
    assert!(!text.contains(PASSWORD), "the record holds no password");
}

#[tokio::test]
async fn wrong_passwords_and_unknown_names_are_refused_alike_and_lock() {
    let (state, app, _) = open_server().await;
    sign_up(&state, &app, "zoe").await;
    let ((status, body), export_key) = opaque_login(&app, "nobody", PASSWORD).await;
    assert_eq!(
        (status, body["error"].as_str(), export_key),
        (StatusCode::UNAUTHORIZED, Some("badCredentials"), None),
        "an unknown name fails like a wrong password"
    );
    for _ in 0..5 {
        let ((status, _), _) = opaque_login(&app, "zoe", "wrong horse").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let start = login_start(PASSWORD).unwrap();
    let (status, body) = call(
        &app,
        "POST",
        "/auth/opaque/login/start",
        None,
        Some(json!({ "username": "zoe", "request": start.request })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::TOO_MANY_REQUESTS, Some("lockedOut"))
    );
    state.clock.advance(2 * MINUTE);
    let ((status, _), _) = opaque_login(&app, "zoe", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "the lock ends");
    assert!(events(&state).await.iter().any(|e| e == "locked_out"));
}

#[tokio::test]
async fn the_kind_of_an_account() {
    let (state, app, _) = open_server().await;
    sign_up(&state, &app, "zoe").await;
    let kind = |name: &'static str| {
        let app = app.clone();
        async move {
            let (_, body) = call(
                &app,
                "POST",
                "/auth/kind",
                None,
                Some(json!({ "username": name })),
            )
            .await;
            body["kind"].as_str().unwrap().to_owned()
        }
    };
    assert_eq!(kind("zoe").await, "e2ee");
    assert_eq!(kind("admin").await, "standard");
    assert_eq!(
        kind("ghost").await,
        kind("ghost").await,
        "the same answer every time"
    );
}

#[tokio::test]
async fn sensitive_changes_need_a_proof_of_the_password() {
    let (state, app, _) = open_server().await;
    let (_, _, account) = sign_up(&state, &app, "zoe").await;
    let token = account.unwrap().token;

    let (status, setup) = call(&app, "POST", "/me/totp/setup", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let secret = setup["secret"].as_str().unwrap().to_owned();
    let code = totp::code_now(&secret, state.clock.now());
    let (status, body) = call(
        &app,
        "POST",
        "/me/totp/confirm",
        Some(&token),
        Some(json!({ "code": code, "password": PASSWORD })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("confirmNeeded")),
        "an e2ee password never travels, even to confirm"
    );
    let (status, body) = proof(&app, &token, "zoe", "wrong horse").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, body) = proof(&app, &token, "zoe", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let proof_token = body["proof"].as_str().unwrap().to_owned();
    let (status, _) = call(
        &app,
        "POST",
        "/me/totp/confirm",
        Some(&token),
        Some(json!({ "code": code, "proof": proof_token })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &app,
        "POST",
        "/me/recovery-codes",
        Some(&token),
        Some(json!({ "proof": proof_token })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a proof works once");

    // the second factor applies to OPAQUE sign-ins too
    state.clock.advance(MINUTE / 2);
    let ((status, needed), _) = opaque_login(&app, "zoe", PASSWORD).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{needed}");
    let code = totp::code_now(&secret, state.clock.now());
    let (status, _) = call(
        &app,
        "POST",
        "/auth/second-factor",
        None,
        Some(json!({ "challenge": needed["challenge"], "method": "totp", "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = call(
        &app,
        "POST",
        "/me/password",
        Some(&token),
        Some(json!({ "current": PASSWORD, "new": "another one" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_recovery_code_sets_a_new_password() {
    let (state, app, _) = open_server().await;
    let (_, _, account) = sign_up(&state, &app, "zoe").await;
    let account = account.unwrap();

    let other = new_account(&account.export_key);
    let wrong = AccountKeys::recovery_verifier(&other.recovery_code).unwrap();
    let (status, body) = call(
        &app,
        "POST",
        "/auth/recovery/start",
        None,
        Some(json!({ "username": "zoe", "verifier": wrong })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("recoveryInvalid"))
    );

    let typed = account.recovery_code.to_lowercase().replace('-', " ");
    let verifier = AccountKeys::recovery_verifier(&typed).unwrap();
    let (status, started) = call(
        &app,
        "POST",
        "/auth/recovery/start",
        None,
        Some(json!({ "username": "zoe", "verifier": verifier })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let bundle: KeyBundle = serde_json::from_value(started["keys"].clone()).unwrap();
    let master = AccountKeys::open_with_recovery(&bundle, &typed).unwrap();
    let (upload, export2) = register_password(&app, "zoe", "fresh horse staple").await;
    let bundle2 = AccountKeys::rewrap_password(&bundle, &master, &export2);
    let finish = json!({ "challenge": started["challenge"], "ksf": 2, "upload": upload, "keys": bundle2, "device": device("new phone") });
    let (status, session) = call(
        &app,
        "POST",
        "/auth/recovery/finish",
        None,
        Some(finish.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let (status, _) = call(&app, "POST", "/auth/recovery/finish", None, Some(finish)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a recovery works once");

    let (status, _) = call(&app, "GET", "/me", Some(&account.token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "every old device is out");
    let ((status, _), export) = opaque_login(&app, "zoe", "fresh horse staple").await;
    assert_eq!(status, StatusCode::OK);
    let reopened = AccountKeys::open_with_password(&bundle2, &export.unwrap()).unwrap();
    assert_eq!(reopened.as_bytes(), master.as_bytes());
    assert!(
        events(&state)
            .await
            .iter()
            .any(|e| e == "password_recovered")
    );
}

/// The stretching number a sign-in as `username` starts with.
async fn ksf_of(app: &Router, username: &str) -> u64 {
    let start = login_start("anything").unwrap();
    let (status, started) = call(
        app,
        "POST",
        "/auth/opaque/login/start",
        None,
        Some(json!({ "username": username, "request": start.request })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    started["ksf"].as_u64().unwrap()
}

#[tokio::test]
async fn an_old_record_is_strengthened_and_keeps_its_keys() {
    let (state, app, _) = open_server().await;
    // an account from before 0.3.0: the old stretching, and no ksf field
    let (upload, export_key) =
        register_password_with(&app, "olga", PASSWORD, app_e2ee::Ksf::V1).await;
    let account = new_account(&export_key);
    let body = super::tests::e2ee_account(&state, "olga", &upload, 1, &account).await;
    let token = body["accessToken"].as_str().unwrap().to_owned();

    // the sign-in says how the record stretches, and opens it
    assert_eq!(ksf_of(&app, "olga").await, 1);
    let ((status, signed), key) = opaque_login(&app, "olga", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{signed}");
    assert_eq!(key.as_deref(), Some(export_key.as_slice()));

    // while an old record remains, an unknown name answers either number,
    // the same each time
    let unknown = ksf_of(&app, "nobody").await;
    assert!(unknown == 1 || unknown == 2);
    assert_eq!(ksf_of(&app, "nobody").await, unknown);

    // registered again with the same password, the master key rewrapped
    let master = AccountKeys::open_with_password(&account.bundle, &export_key).unwrap();
    let (upload, strong_key) = register_password(&app, "olga", PASSWORD).await;
    let keys = AccountKeys::rewrap_password(&account.bundle, &master, &strong_key);
    let mut tampered = keys.clone();
    tampered.by_recovery = keys.by_password.clone();
    let (_, confirmed) = proof(&app, &token, "olga", PASSWORD).await;
    let (status, _) = call(
        &app,
        "POST",
        "/me/e2ee/strengthen",
        Some(&token),
        Some(json!({ "upload": upload, "keys": tampered, "proof": confirmed["proof"], "ksf": 2 })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the recovery wrapping stays"
    );
    // past the sign-in rate limit the confirmations count towards
    state.clock.advance(MINUTE);
    let (status, confirmed) = proof(&app, &token, "olga", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    let (status, body) = call(
        &app,
        "POST",
        "/me/e2ee/strengthen",
        Some(&token),
        Some(json!({ "upload": upload, "keys": keys, "proof": confirmed["proof"], "ksf": 2 })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    // the session stays, the new record signs in and opens the same keys
    let (status, stored) = call(&app, "GET", "/me/keys", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert_eq!(ksf_of(&app, "olga").await, 2);
    let ((status, _), key) = opaque_login(&app, "olga", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(key.as_deref(), Some(strong_key.as_slice()));
    let bundle: KeyBundle = serde_json::from_value(stored["keys"].clone()).unwrap();
    let opened = AccountKeys::open_with_password(&bundle, &strong_key).unwrap();
    assert_eq!(opened.as_bytes(), master.as_bytes());
    assert!(
        events(&state)
            .await
            .iter()
            .any(|e| e == "password_strengthened")
    );

    // no old record left: unknown names say the current number
    assert_eq!(ksf_of(&app, "nobody").await, 2);

    // never again at the same strength, nor weaker
    for ksf in [2, 1] {
        let (_, confirmed) = proof(&app, &token, "olga", PASSWORD).await;
        let (upload, again) = register_password(&app, "olga", PASSWORD).await;
        let keys = AccountKeys::rewrap_password(&bundle, &master, &again);
        let (status, body) = call(
            &app,
            "POST",
            "/me/e2ee/strengthen",
            Some(&token),
            Some(
                json!({ "upload": upload, "keys": keys, "proof": confirmed["proof"], "ksf": ksf }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
    }
    // a number nobody knows is refused before anything
    let (status, _) = call(
        &app,
        "POST",
        "/me/e2ee/strengthen",
        Some(&token),
        Some(json!({ "upload": "AA", "keys": bundle, "proof": "x", "ksf": 9 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
