// SPDX-License-Identifier: AGPL-3.0-or-later

//! Passkeys through the full router, with a software authenticator playing
//! the browser's part.

use axum::Router;
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use webauthn_authenticator_rs::WebauthnAuthenticator;
use webauthn_authenticator_rs::softpasskey::SoftPasskey;
use webauthn_rs::prelude::{Url, Uuid};

use super::tests::{admin_on, call, device, events, login, server_with, with_admin};
use crate::config::Config;
use crate::routes::AppState;

const ORIGIN: &str = "https://notes.example.org";
const PASSWORD: &str = "admin password";

type Authenticator = WebauthnAuthenticator<SoftPasskey>;

fn origin() -> Url {
    Url::parse(ORIGIN).unwrap()
}

async fn with_passkeys() -> (AppState, Router, String) {
    let mut config = Config::default();
    config.server.public_url = ORIGIN.into();
    admin_on(server_with(config).await).await
}

/// Adds a passkey held by `key`, returns the answer.
async fn add(
    app: &Router,
    token: &str,
    key: &mut Authenticator,
    name: &str,
) -> (StatusCode, Value) {
    let (status, ceremony) = call(
        app,
        "POST",
        "/me/passkeys/start",
        Some(token),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ceremony}");
    let options = serde_json::from_value(ceremony["options"].clone()).unwrap();
    let credential = key.do_registration(origin(), options).unwrap();
    call(
        app,
        "POST",
        "/me/passkeys/finish",
        Some(token),
        Some(json!({ "id": ceremony["id"], "credential": credential, "name": name })),
    )
    .await
}

/// The authenticator's answer to sign-in options.
fn answer(key: &mut Authenticator, options: &Value) -> Value {
    let options = serde_json::from_value(options.clone()).unwrap();
    serde_json::to_value(key.do_authentication(origin(), options).unwrap()).unwrap()
}

async fn passkey_sign_in(
    app: &Router,
    username: &str,
    key: &mut Authenticator,
) -> (StatusCode, Value) {
    let (status, ceremony) = call(
        app,
        "POST",
        "/auth/passkey/start",
        None,
        Some(json!({ "username": username })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ceremony}");
    let options = serde_json::from_value(ceremony["options"].clone()).unwrap();
    let credential = match key.do_authentication(origin(), options) {
        Ok(c) => serde_json::to_value(c).unwrap(),
        // nothing the authenticator holds was asked for
        Err(_) => json!({}),
    };
    call(
        app,
        "POST",
        "/auth/passkey/finish",
        None,
        Some(json!({ "id": ceremony["id"], "credential": credential, "device": device("phone") })),
    )
    .await
}

/// Signs in with no username, as a browser does with a passkey it holds:
/// `registered` is the answer that made the passkey, `user_id` the account
/// the authenticator stored with it.
pub(super) async fn discoverable_sign_in(
    app: &Router,
    key: &mut Authenticator,
    registered: &Value,
    user_id: &str,
) -> (StatusCode, Value) {
    let (status, ceremony) = call(app, "POST", "/auth/passkey/start", None, Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{ceremony}");
    let mut options = ceremony["options"].clone();
    assert!(options.get("mediation").is_none(), "{options}");
    assert!(
        options["publicKey"]["allowCredentials"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "no account named"
    );
    // a browser finds its passkey by itself, the software one needs its id
    options["publicKey"]["allowCredentials"] =
        json!([{ "type": "public-key", "id": registered["id"] }]);
    let mut credential = answer(key, &options);
    // and a stored passkey answers with the account it was made for
    let handle = Uuid::parse_str(user_id).unwrap();
    credential["response"]["userHandle"] = json!(URL_SAFE_NO_PAD.encode(handle.as_bytes()));
    call(
        app,
        "POST",
        "/auth/passkey/finish",
        None,
        Some(json!({ "id": ceremony["id"], "credential": credential, "device": device("phone") })),
    )
    .await
}

#[tokio::test]
async fn a_passkey_signs_in_without_a_username() {
    let (_, app, token) = with_passkeys().await;
    let (status, ceremony) = call(
        &app,
        "POST",
        "/me/passkeys/start",
        Some(&token),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        ceremony["options"]["publicKey"]["authenticatorSelection"]["residentKey"], "required",
        "the authenticator keeps the account"
    );
    let mut key = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let options = serde_json::from_value(ceremony["options"].clone()).unwrap();
    let registered = serde_json::to_value(key.do_registration(origin(), options).unwrap()).unwrap();
    let (status, _) = call(
        &app,
        "POST",
        "/me/passkeys/finish",
        Some(&token),
        Some(json!({ "id": ceremony["id"], "credential": registered, "name": "Laptop" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, me) = call(&app, "GET", "/me", Some(&token), None).await;
    let id = me["id"].as_str().unwrap();

    let (status, session) = discoverable_sign_in(&app, &mut key, &registered, id).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["user"]["username"], "admin");

    let stranger = Uuid::now_v7().to_string();
    let (status, body) = discoverable_sign_in(&app, &mut key, &registered, &stranger).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("passkeyInvalid")),
        "the passkey names an account that is not its own"
    );
}

#[tokio::test]
async fn without_a_public_address_passkeys_are_off() {
    let (_, app, token) = with_admin().await;
    let (_, options) = call(&app, "GET", "/auth/options", None, None).await;
    assert_eq!(options["passkeys"], false);
    let (status, body) = call(
        &app,
        "POST",
        "/me/passkeys/start",
        Some(&token),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("passkeysUnavailable"))
    );
}

#[tokio::test]
async fn a_passkey_signs_in_alone() {
    let (state, app, token) = with_passkeys().await;
    let (_, options) = call(&app, "GET", "/auth/options", None, None).await;
    assert_eq!(options["passkeys"], true);
    let mut key = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let (status, added) = add(&app, &token, &mut key, " Blue key ").await;
    assert_eq!(status, StatusCode::CREATED, "{added}");
    assert_eq!(added["passkey"]["name"], "Blue key");
    assert_eq!(added["codes"].as_array().unwrap().len(), 10, "first factor");

    let (status, session) = passkey_sign_in(&app, "ADMIN", &mut key).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert!(session["accessToken"].as_str().is_some());

    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    let passkeys = security["passkeys"].as_array().unwrap();
    assert_eq!(passkeys.len(), 1);
    assert!(passkeys[0]["lastUsedAt"].as_i64().is_some());
    let log = events(&state).await;
    assert!(log.iter().any(|e| e == "passkey_added"));
}

#[tokio::test]
async fn another_key_or_another_account_does_not_sign_in() {
    let (_, app, token) = with_passkeys().await;
    let mut key = WebauthnAuthenticator::new(SoftPasskey::new(true));
    add(&app, &token, &mut key, "Phone").await;

    let mut stranger = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let (status, body) = passkey_sign_in(&app, "admin", &mut stranger).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("passkeyInvalid"))
    );
    // an unknown account gets options of the same shape, and a refusal
    let (status, _) = passkey_sign_in(&app, "nobody", &mut key).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_passkey_is_also_the_second_factor_after_a_password() {
    let (_, app, token) = with_passkeys().await;
    let mut key = WebauthnAuthenticator::new(SoftPasskey::new(true));
    add(&app, &token, &mut key, "Phone").await;

    let (status, needed) = login(&app, "admin", PASSWORD).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(needed["methods"], json!(["passkey", "recoveryCode"]));
    let credential = answer(&mut key, &needed["passkey"]);
    let (status, session) = call(
        &app,
        "POST",
        "/auth/second-factor",
        None,
        Some(
            json!({ "challenge": needed["challenge"], "method": "passkey", "passkey": credential }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
}

#[tokio::test]
async fn removing_the_last_passkey_drops_the_second_step() {
    let (_, app, token) = with_passkeys().await;
    let mut key = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let (_, added) = add(&app, &token, &mut key, "Phone").await;
    let id = added["passkey"]["id"].as_str().unwrap();

    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/me/passkeys/{id}"),
        Some(&token),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "needs the password");
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/me/passkeys/{id}"),
        Some(&token),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    assert_eq!(security["recoveryCodesLeft"], 0);
    let (status, _) = login(&app, "admin", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
}
