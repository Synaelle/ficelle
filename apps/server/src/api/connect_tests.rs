// SPDX-License-Identifier: AGPL-3.0-or-later

//! An app signing in through the server's page: the code, its one try, its
//! expiry, the verifier and the redirect it is bound to.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::tests::{call, with_admin};

/// RFC 7636 appendix B.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const REDIRECT: &str = "http://127.0.0.1:53111/callback";

fn ask(redirect: &str, challenge: &str) -> Value {
    json!({
        "redirect": redirect,
        "challenge": challenge,
        "device": { "name": "Ficelle on laptop", "platform": "linux" },
        "sealed": "c2VhbGVkIGtleQ",
    })
}

async fn code(app: &axum::Router, page: &str) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/auth/app/code",
        Some(page),
        Some(ask(REDIRECT, CHALLENGE)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["code"].as_str().unwrap().to_owned()
}

fn trade(code: &str, verifier: &str, redirect: &str) -> Value {
    json!({ "code": code, "verifier": verifier, "redirect": redirect })
}

#[tokio::test]
async fn the_app_gets_its_own_session_once() {
    let (_state, app, page) = with_admin().await;
    let code = code(&app, &page).await;
    assert!(code.starts_with("ac_"));
    let (status, body) = call(
        &app,
        "POST",
        "/auth/app/token",
        None,
        Some(trade(&code, VERIFIER, REDIRECT)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sealed"], "c2VhbGVkIGtleQ", "passed along untouched");
    let token = body["session"]["accessToken"].as_str().unwrap();
    assert_ne!(token, page, "a session of its own");
    let (status, me) = call(&app, "GET", "/me", Some(token), None).await;
    assert_eq!((status, &me["username"]), (StatusCode::OK, &json!("admin")));

    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/token",
        None,
        Some(trade(&code, VERIFIER, REDIRECT)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a code works once");
}

#[tokio::test]
async fn a_wrong_try_ends_the_code() {
    let (_state, app, page) = with_admin().await;
    let code = code(&app, &page).await;
    let wrong = "x".repeat(43);
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/token",
        None,
        Some(trade(&code, &wrong, REDIRECT)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "another verifier");
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/token",
        None,
        Some(trade(&code, VERIFIER, REDIRECT)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no second try");

    let code = super::connect_tests::code(&app, &page).await;
    let elsewhere = "http://127.0.0.1:53112/callback";
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/token",
        None,
        Some(trade(&code, VERIFIER, elsewhere)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "another redirect");
}

#[tokio::test]
async fn a_code_lasts_two_minutes() {
    let (state, app, page) = with_admin().await;
    let code = code(&app, &page).await;
    state.clock.advance(2 * 60 * 1000 + 1);
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/token",
        None,
        Some(trade(&code, VERIFIER, REDIRECT)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn only_a_signed_in_page_and_the_apps_own_addresses() {
    let (_state, app, page) = with_admin().await;
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/code",
        None,
        Some(ask(REDIRECT, CHALLENGE)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for (redirect, challenge) in [
        ("https://evil.example/callback", CHALLENGE),
        ("com.evil.app:/connect", CHALLENGE),
        (REDIRECT, "plain"),
    ] {
        let (status, _) = call(
            &app,
            "POST",
            "/auth/app/code",
            Some(&page),
            Some(ask(redirect, challenge)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{redirect} {challenge}");
    }
    let (status, info) = call(&app, "GET", "/info", None, None).await;
    assert_eq!(
        (status, &info["signInPage"]),
        (StatusCode::OK, &json!(true))
    );
}
