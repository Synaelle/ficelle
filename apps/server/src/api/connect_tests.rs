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

const KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

async fn link_code(app: &axum::Router) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/auth/app/link",
        None,
        Some(json!({
            "challenge": CHALLENGE,
            "key": KEY,
            "device": { "name": "Ficelle on IN2013", "platform": "android" },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["interval"], 3);
    body["code"].as_str().unwrap().to_owned()
}

async fn ask_link(app: &axum::Router, code: &str, verifier: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/auth/app/link/token",
        None,
        Some(json!({ "code": code, "verifier": verifier })),
    )
    .await
}

#[tokio::test]
async fn a_code_typed_on_the_page_signs_the_app_in_once() {
    let (_state, app, page) = with_admin().await;
    let code = link_code(&app).await;
    assert_eq!(code.len(), 9, "{code}");
    let (status, body) = ask_link(&app, &code, VERIFIER).await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::BAD_REQUEST, &json!("linkPending"))
    );

    // typed in lower case without the dash, before signing in
    let typed = code.replace('-', "").to_lowercase();
    let (status, info) = call(
        &app,
        "POST",
        "/auth/app/link/find",
        None,
        Some(json!({ "code": typed })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{info}");
    assert_eq!(info["device"]["name"], "Ficelle on IN2013");
    assert_eq!(
        (&info["key"], &info["challenge"]),
        (&json!(KEY), &json!(CHALLENGE))
    );

    let approve = json!({ "code": typed, "sealed": "c2VhbGVkIGtleQ" });
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/link/approve",
        None,
        Some(approve.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "only signed in");
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/link/approve",
        Some(&page),
        Some(approve.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/link/approve",
        Some(&page),
        Some(approve),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "approved once");

    let wrong = "y".repeat(43);
    let (status, body) = ask_link(&app, &code, &wrong).await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::BAD_REQUEST, &json!("linkInvalid")),
        "another verifier hears nothing"
    );
    let (status, body) = ask_link(&app, &code, VERIFIER).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sealed"], "c2VhbGVkIGtleQ");
    let token = body["session"]["accessToken"].as_str().unwrap();
    let (status, me) = call(&app, "GET", "/me", Some(token), None).await;
    assert_eq!((status, &me["username"]), (StatusCode::OK, &json!("admin")));
    let (status, _) = ask_link(&app, &code, VERIFIER).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a session once");
}

#[tokio::test]
async fn a_code_lasts_ten_minutes_and_needs_a_real_request() {
    let (state, app, page) = with_admin().await;
    let code = link_code(&app).await;
    state.clock.advance(10 * 60 * 1000 + 1);
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/link/find",
        None,
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/link/approve",
        Some(&page),
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = ask_link(&app, &code, VERIFIER).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    for bad in [
        json!({ "challenge": "plain", "key": KEY, "device": { "name": "x", "platform": "linux" } }),
        json!({ "challenge": CHALLENGE, "key": "not a key", "device": { "name": "x", "platform": "linux" } }),
        json!({ "challenge": CHALLENGE, "key": KEY, "device": { "name": " ", "platform": "linux" } }),
    ] {
        let (status, _) = call(&app, "POST", "/auth/app/link", None, Some(bad.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
}

async fn with_address() -> (crate::routes::AppState, axum::Router, String) {
    let mut config = crate::config::Config::default();
    config.server.public_url = "https://notes.example.org".into();
    super::tests::admin_on(super::tests::server_with(config).await).await
}

/// The page's QR code: its token, after `#offer=`.
async fn offer(app: &axum::Router) -> String {
    let (status, body) = call(app, "POST", "/auth/app/link/offer", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let link = body["link"].as_str().unwrap();
    assert!(body["qr"].as_str().unwrap().contains("<svg"));
    link.strip_prefix("https://notes.example.org/link#offer=")
        .unwrap_or_else(|| panic!("{link}"))
        .to_owned()
}

/// A phone that scanned `offer` asks for its code.
async fn scan(app: &axum::Router, offer: &str, name: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/auth/app/link",
        None,
        Some(json!({
            "challenge": CHALLENGE,
            "key": KEY,
            "device": { "name": name, "platform": "android" },
            "offer": offer,
        })),
    )
    .await
}

async fn offer_status(app: &axum::Router, offer: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/auth/app/link/offer/status",
        None,
        Some(json!({ "offer": offer })),
    )
    .await
}

#[tokio::test]
async fn a_scanned_qr_code_signs_the_phone_in_once_approved() {
    let (_state, app, page) = with_address().await;
    let offer = offer(&app).await;
    assert!(offer.starts_with("lo_"), "{offer}");
    let (status, body) = offer_status(&app, &offer).await;
    assert_eq!((status, &body), (StatusCode::OK, &json!({})), "no app yet");

    let (status, body) = scan(&app, &offer, "Ficelle on Pixel 8").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let code = body["code"].as_str().unwrap().to_owned();
    let (status, body) = ask_link(&app, &code, VERIFIER).await;
    assert_eq!(body["error"], "linkPending", "{status}");

    let (status, body) = offer_status(&app, &offer).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["app"]["device"]["name"], "Ficelle on Pixel 8");
    assert_eq!(
        (&body["app"]["key"], &body["app"]["challenge"]),
        (&json!(KEY), &json!(CHALLENGE))
    );

    let approve = json!({ "offer": offer, "sealed": "c2VhbGVkIGtleQ" });
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/link/offer/approve",
        None,
        Some(approve.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "only signed in");
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/link/offer/approve",
        Some(&page),
        Some(approve.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/link/offer/approve",
        Some(&page),
        Some(approve),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "approved once");
    let (status, _) = offer_status(&app, &offer).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "the QR code is used up");

    let (status, body) = ask_link(&app, &code, VERIFIER).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sealed"], "c2VhbGVkIGtleQ");
    let token = body["session"]["accessToken"].as_str().unwrap();
    let (status, me) = call(&app, "GET", "/me", Some(token), None).await;
    assert_eq!((status, &me["username"]), (StatusCode::OK, &json!("admin")));
}

#[tokio::test]
async fn a_qr_code_takes_one_phone_and_lasts_five_minutes() {
    let (state, app, page) = with_address().await;
    let first = offer(&app).await;
    let (status, _) = scan(&app, &first, "Ficelle on Pixel 8").await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = scan(&app, &first, "Ficelle on someone else's phone").await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::BAD_REQUEST, &json!("linkInvalid")),
        "a second scan gets nothing"
    );
    let (_, body) = offer_status(&app, &first).await;
    assert_eq!(
        body["app"]["device"]["name"], "Ficelle on Pixel 8",
        "the first stays"
    );

    let (status, _) = scan(&app, "lo_made-up", "Ficelle on Pixel 8").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "an unknown QR code");

    let second = offer(&app).await;
    state.clock.advance(5 * 60 * 1000 + 1);
    let (status, _) = scan(&app, &second, "Ficelle on Pixel 8").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "expired");
    let (status, _) = offer_status(&app, &second).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "expired for the page too");
    let (status, _) = call(
        &app,
        "POST",
        "/auth/app/link/offer/approve",
        Some(&page),
        Some(json!({ "offer": first })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "nothing left to approve");
}

#[tokio::test]
async fn a_qr_code_needs_the_servers_address() {
    let (_state, app, _page) = with_admin().await;
    let (status, body) = call(&app, "POST", "/auth/app/link/offer", None, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn one_network_shows_a_few_qr_codes_at_once() {
    let (_state, app, _page) = with_address().await;
    for _ in 0..5 {
        offer(&app).await;
    }
    let (status, body) = call(&app, "POST", "/auth/app/link/offer", None, None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
}
