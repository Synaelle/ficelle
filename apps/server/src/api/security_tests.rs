// SPDX-License-Identifier: AGPL-3.0-or-later

//! Second factors through the full router.

use axum::Router;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::tests::{call, events, login, with_admin};
use crate::accounts::totp;
use crate::clock::MINUTE;
use crate::routes::AppState;

const PASSWORD: &str = "admin password";

/// Turns the authenticator app on for the admin, returns its secret and
/// the recovery codes.
async fn with_totp() -> (AppState, Router, String, String, Vec<String>) {
    let (state, app, token) = with_admin().await;
    let (status, setup) = call(&app, "POST", "/me/totp/setup", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    let secret = setup["secret"].as_str().unwrap().to_owned();
    assert!(
        setup["uri"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/")
    );
    assert!(setup["qrSvg"].as_str().unwrap().contains("<svg"));

    let code = totp::code_now(&secret, state.clock.now());
    let (status, body) = call(
        &app,
        "POST",
        "/me/totp/confirm",
        Some(&token),
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("confirmNeeded")),
        "the password is needed"
    );
    let (status, body) = call(
        &app,
        "POST",
        "/me/totp/confirm",
        Some(&token),
        Some(json!({ "code": code, "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let codes: Vec<String> = body["codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(codes.len(), totp::RECOVERY_CODES);
    // the next code, the confirming one cannot sign in again
    state.clock.advance(MINUTE / 2);
    (state, app, token, secret, codes)
}

async fn second_step(
    app: &Router,
    challenge: &str,
    method: &str,
    code: &str,
) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/auth/second-factor",
        None,
        Some(json!({ "challenge": challenge, "method": method, "code": code })),
    )
    .await
}

#[tokio::test]
async fn a_wrong_first_code_keeps_the_app_off() {
    let (_, app, token) = with_admin().await;
    call(&app, "POST", "/me/totp/setup", Some(&token), None).await;
    let (status, body) = call(
        &app,
        "POST",
        "/me/totp/confirm",
        Some(&token),
        Some(json!({ "code": "000000", "password": PASSWORD })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("totpInvalid"))
    );
    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    assert_eq!(security["totp"], false);
    let (status, _) = login(&app, "admin", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "no second step");
}

#[tokio::test]
async fn sign_in_asks_for_the_code_after_the_password() {
    let (state, app, token, secret, _) = with_totp().await;
    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    assert_eq!(
        (
            security["totp"].as_bool(),
            security["recoveryCodesLeft"].as_u64()
        ),
        (Some(true), Some(10))
    );

    let (status, needed) = login(&app, "admin", PASSWORD).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{needed}");
    assert_eq!(needed["methods"], json!(["totp", "recoveryCode"]));
    assert!(needed.get("accessToken").is_none(), "no session yet");
    let challenge = needed["challenge"].as_str().unwrap();

    let (status, body) = second_step(&app, challenge, "totp", "000000").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("secondFactorInvalid"))
    );
    let code = totp::code_now(&secret, state.clock.now());
    let (status, session) = second_step(&app, challenge, "totp", &code).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert!(session["accessToken"].as_str().is_some());

    let (status, _) = second_step(&app, challenge, "totp", &code).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a challenge works once");
    let (_, again) = login(&app, "admin", PASSWORD).await;
    let (status, _) = second_step(&app, again["challenge"].as_str().unwrap(), "totp", &code).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a code works once");

    let log = events(&state).await;
    assert!(log.iter().any(|e| e == "second_factor_failed"));
    assert!(log.iter().any(|e| e == "totp_enabled"));
}

#[tokio::test]
async fn a_challenge_expires_and_takes_five_answers() {
    let (state, app, _, secret, _) = with_totp().await;
    let (_, needed) = login(&app, "admin", PASSWORD).await;
    let challenge = needed["challenge"].as_str().unwrap().to_owned();
    state.clock.advance(6 * MINUTE);
    let code = totp::code_now(&secret, state.clock.now());
    let (status, _) = second_step(&app, &challenge, "totp", &code).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "expired");

    let (_, needed) = login(&app, "admin", PASSWORD).await;
    let challenge = needed["challenge"].as_str().unwrap().to_owned();
    for _ in 0..4 {
        second_step(&app, &challenge, "totp", "000000").await;
    }
    // the fifth wrong answer in a row locks the account, like passwords
    let (status, body) = second_step(&app, &challenge, "totp", "000000").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, body) = second_step(&app, &challenge, "totp", &code).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, body) = login(&app, "admin", PASSWORD).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::TOO_MANY_REQUESTS, Some("lockedOut"))
    );
}

#[tokio::test]
async fn recovery_codes_work_once_each_in_any_case() {
    let (state, app, token, _, codes) = with_totp().await;
    let (_, needed) = login(&app, "admin", PASSWORD).await;
    let challenge = needed["challenge"].as_str().unwrap();
    let typed = codes[0].to_uppercase().replace('-', " ");
    let (status, _) = second_step(&app, challenge, "recoveryCode", &typed).await;
    assert_eq!(status, StatusCode::OK);

    let (_, needed) = login(&app, "admin", PASSWORD).await;
    let challenge = needed["challenge"].as_str().unwrap();
    let (status, _) = second_step(&app, challenge, "recoveryCode", &codes[0]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "spent");
    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    assert_eq!(security["recoveryCodesLeft"], 9);

    let (status, fresh) = call(
        &app,
        "POST",
        "/me/recovery-codes",
        Some(&token),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fresh["codes"].as_array().unwrap().len(), 10);
    let (status, _) = second_step(&app, challenge, "recoveryCode", &codes[1]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "the old set is gone");
    let log = events(&state).await;
    assert!(log.iter().any(|e| e == "recovery_codes_created"));
}

#[tokio::test]
async fn turning_the_app_off_needs_the_password_and_drops_the_codes() {
    let (_, app, token, _, _) = with_totp().await;
    let (status, body) = call(
        &app,
        "POST",
        "/me/totp/disable",
        Some(&token),
        Some(json!({ "password": "wrong password" })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("badCredentials"))
    );
    let (status, _) = call(
        &app,
        "POST",
        "/me/totp/disable",
        Some(&token),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    assert_eq!(
        (
            security["totp"].as_bool(),
            security["recoveryCodesLeft"].as_u64()
        ),
        (Some(false), Some(0))
    );
    let (status, _) = login(&app, "admin", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &app,
        "POST",
        "/me/recovery-codes",
        Some(&token),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "no codes without a factor");
}

#[tokio::test]
async fn an_admin_resets_a_lost_second_factor() {
    let (state, app, token, _, _) = with_totp().await;
    let (_, me) = call(&app, "GET", "/me", Some(&token), None).await;
    let id = me["id"].as_str().unwrap();
    let (_, user) = call(
        &app,
        "GET",
        &format!("/admin/users/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(user["user"]["secondFactor"], true);
    let (status, _) = call(
        &app,
        "POST",
        &format!("/admin/users/{id}/second-factors/reset"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, "GET", "/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "devices signed out");
    let (status, session) = login(&app, "admin", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "the password alone again");
    let token = session["accessToken"].as_str().unwrap();
    let (_, security) = call(&app, "GET", "/me/security", Some(token), None).await;
    assert_eq!(security["recoveryCodesLeft"], 0);
    assert!(
        events(&state)
            .await
            .iter()
            .any(|e| e == "second_factors_reset")
    );
}

/// A stolen session cannot guess the password
/// through re-confirmations faster than a sign-in could.
#[tokio::test]
async fn confirming_the_password_locks_like_a_sign_in() {
    let (_state, app, token) = super::tests::with_admin().await;
    let wrong = || json!({ "current": "not it", "new": "another long password" });
    for _ in 0..5 {
        let (status, body) = call(&app, "POST", "/me/password", Some(&token), Some(wrong())).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    }
    for path in ["/me/password", "/me/totp/disable"] {
        let body = if path == "/me/password" {
            json!({ "current": PASSWORD, "new": "another long password" })
        } else {
            json!({ "password": PASSWORD })
        };
        let (status, body) = call(&app, "POST", path, Some(&token), Some(body)).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (StatusCode::TOO_MANY_REQUESTS, Some("lockedOut")),
            "{path}: even the right password waits"
        );
    }
}
