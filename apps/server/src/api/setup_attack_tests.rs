// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trying to take over a server that is being set up: racing the link,
//! guessing it, guessing or reusing the session, the command line making the
//! admin while the wizard is open, a restart after the lock, every api route
//! and a few spellings of them while setup is pending.

use std::collections::BTreeSet;
use std::net::SocketAddr;

use app_protocol::{PROTOCOL_HEADER, PROTOCOL_VERSION};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::tests::{call, device};
use crate::accounts::totp;
use crate::config::{AuthConfig, Config};
use crate::db::Db;
use crate::routes::{AppState, router};

fn config() -> Config {
    let mut config = Config {
        auth: AuthConfig {
            argon2_memory_kib: 64,
            argon2_iterations: 1,
            ..AuthConfig::default()
        },
        ..Config::default()
    };
    config.setup.allow_http = true;
    // passkeys need an address, every setup call then exists
    config.server.public_url = "https://notes.example.org".into();
    config
}

fn fresh() -> (AppState, Router) {
    let state = AppState::new(config(), Db::in_memory().unwrap());
    let app = router(state.clone());
    (state, app)
}

/// A request from `peer`, as the network delivers it.
async fn send(
    app: &Router,
    peer: &str,
    method: &str,
    uri: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string());
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    let mut req = req
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(peer.parse().unwrap(), 40_000)));
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn start_from(app: &Router, peer: &str, token: &str) -> (StatusCode, Value) {
    send(
        app,
        peer,
        "POST",
        "/api/v1/setup/start",
        Some(&json!({ "token": token })),
    )
    .await
}

#[tokio::test]
async fn a_link_raced_from_many_places_opens_one_session() {
    let (state, app) = fresh();
    let token = state.setup.new_link(state.clock.now());
    let mut racers = tokio::task::JoinSet::new();
    for i in 0..40 {
        let app = app.clone();
        let token = token.clone();
        racers.spawn(async move { start_from(&app, &format!("198.51.100.{i}"), &token).await });
    }
    let mut won = Vec::new();
    while let Some(result) = racers.join_next().await {
        let (status, body) = result.unwrap();
        match status {
            StatusCode::OK => won.push(body["session"].as_str().unwrap().to_owned()),
            StatusCode::FORBIDDEN => assert_eq!(body["error"], "setupLinkInvalid"),
            other => panic!("{other} {body}"),
        }
    }
    assert_eq!(won.len(), 1, "one session, whoever came first");
}

#[tokio::test]
async fn guessing_the_link_runs_into_the_rate_limit() {
    let (state, app) = fresh();
    let token = state.setup.new_link(state.clock.now());
    let mut limited = false;
    for i in 0..30 {
        let (status, body) = start_from(&app, "203.0.113.9", &format!("sl_guess{i}")).await;
        if status == StatusCode::TOO_MANY_REQUESTS {
            limited = true;
            break;
        }
        assert_eq!(body["error"], "setupLinkInvalid");
    }
    assert!(limited, "guesses are slowed down");
    let (status, _) = start_from(&app, "203.0.113.9", &token).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "even the right one");
    // the guesses did not burn the link for the operator elsewhere
    let (status, body) = start_from(&app, "192.0.2.1", &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// Every setup call that takes a session, with this session.
async fn each_session_call(app: &Router, peer: &str, session: &str) -> Vec<(String, StatusCode)> {
    let mut out = Vec::new();
    let body = json!({ "session": session, "username": "root" });
    for path in ["totp", "passkey", "checks"] {
        let (status, _) = send(
            app,
            peer,
            "POST",
            &format!("/api/v1/setup/{path}"),
            Some(&body),
        )
        .await;
        out.push((path.to_owned(), status));
    }
    let finish = json!({
        "session": session,
        "username": "root",
        "password": "a long admin password",
        "factor": { "type": "totp", "code": "000000" },
        "device": device("laptop"),
    });
    let (status, _) = send(app, peer, "POST", "/api/v1/setup/finish", Some(&finish)).await;
    out.push(("finish".into(), status));
    let mut req = Request::post("/api/v1/setup/upload")
        .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
        .header("x-setup-session", session)
        .body(Body::from(vec![0u8; 1024]))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(peer.parse().unwrap(), 40_000)));
    let status = app.clone().oneshot(req).await.unwrap().status();
    out.push(("upload".into(), status));
    out
}

#[tokio::test]
async fn a_session_cannot_be_guessed_and_dies_with_the_lock() {
    let (state, app) = fresh();
    let token = state.setup.new_link(state.clock.now());
    let (_, started) = start_from(&app, "192.0.2.1", &token).await;
    let session = started["session"].as_str().unwrap().to_owned();

    // many wrong sessions, each from its own address: refused, and they do
    // not count against the real session's five wrong answers
    for i in 0..8 {
        let forged = format!("su_{}", "A".repeat(43 - i));
        for (path, status) in each_session_call(&app, &format!("198.51.100.{i}"), &forged).await {
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{path} with a forged session"
            );
        }
    }
    let (status, body) = send(
        &app,
        "192.0.2.1",
        "POST",
        "/api/v1/setup/totp",
        Some(&json!({ "session": session, "username": "root" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let secret = body["secret"].as_str().unwrap().to_owned();
    let finish = json!({
        "session": session,
        "username": "root",
        "password": "a long admin password",
        "factor": { "type": "totp", "code": totp::code_now(&secret, state.clock.now()) },
        "device": device("laptop"),
    });
    let (status, body) = send(
        &app,
        "192.0.2.1",
        "POST",
        "/api/v1/setup/finish",
        Some(&finish),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // the same session afterwards, from the same place: gone everywhere
    for (path, status) in each_session_call(&app, "192.0.2.2", &session).await {
        assert_eq!(status, StatusCode::GONE, "{path} after the lock");
    }
    let (status, _) = start_from(&app, "192.0.2.2", &state.setup.new_link(state.clock.now())).await;
    assert_eq!(status, StatusCode::GONE, "a link made after the lock");
}

#[tokio::test]
async fn the_command_line_wins_over_an_open_wizard() {
    let (state, app) = fresh();
    let token = state.setup.new_link(state.clock.now());
    let (_, started) = start_from(&app, "192.0.2.1", &token).await;
    let session = started["session"].as_str().unwrap().to_owned();
    let (_, body) = send(
        &app,
        "192.0.2.1",
        "POST",
        "/api/v1/setup/totp",
        Some(&json!({ "session": session, "username": "intruder" })),
    )
    .await;
    let secret = body["secret"].as_str().unwrap().to_owned();

    // the operator makes the admin from a terminal meanwhile
    let first = crate::cli::create_admin(
        &state.db,
        &state.config,
        "operator",
        None,
        "the operator's long password",
        crate::cli::Origin::Command,
    )
    .await
    .unwrap();
    assert!(first, "the command line locked setup");

    let finish = json!({
        "session": session,
        "username": "intruder",
        "password": "a long admin password",
        "factor": { "type": "totp", "code": totp::code_now(&secret, state.clock.now()) },
        "device": device("laptop"),
    });
    let (status, body) = send(
        &app,
        "192.0.2.1",
        "POST",
        "/api/v1/setup/finish",
        Some(&finish),
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "{body}");
    let names: Vec<String> = state
        .db
        .call(|c| {
            let mut stmt = c.prepare("SELECT username FROM users")?;
            Ok(stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
        .unwrap();
    assert_eq!(names, ["operator"]);
    assert!(state.setup.is_done(), "the server noticed");
    // and the other api routes open without a restart
    let (status, _) = call(&app, "GET", "/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_restart_after_the_lock_opens_no_setup() {
    let dir = tempfile::tempdir().unwrap();
    {
        let state = AppState::new(config(), Db::open(dir.path()).unwrap());
        crate::cli::create_admin(
            &state.db,
            &state.config,
            "operator",
            None,
            "the operator's long password",
            crate::cli::Origin::Command,
        )
        .await
        .unwrap();
    }
    let state = AppState::new(config(), Db::open(dir.path()).unwrap());
    let app = router(state.clone());
    // even with a link in memory, as a confused start might make
    let token = state.setup.new_link(state.clock.now());
    let (status, body) = start_from(&app, "127.0.0.1", &token).await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::GONE, &json!("setupDone"))
    );
    let (status, _) = call(&app, "GET", "/setup", None, None).await;
    assert_eq!(status, StatusCode::GONE);
    let (status, _) = call(&app, "GET", "/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "the api is open");
}

/// Every route `api/mod.rs` declares, as (method, path under `/api/v1`),
/// read from the source so a route added later is checked too.
fn declared_routes() -> BTreeSet<(String, String)> {
    let source = include_str!("mod.rs");
    let mut routes = BTreeSet::new();
    for chunk in source.split(".route(").skip(1) {
        // the last route of a function runs until the function ends
        let chunk = chunk.split("\n}").next().unwrap();
        let Some((_, rest)) = chunk.split_once('"') else {
            continue;
        };
        let Some((path, handlers)) = rest.split_once('"') else {
            continue;
        };
        for method in ["get", "post", "put", "patch", "delete"] {
            let call = format!("{method}(");
            let found = handlers.match_indices(&call).any(|(at, _)| {
                !handlers[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':')
            });
            if found {
                routes.insert((method.to_uppercase(), path.to_owned()));
            }
        }
    }
    routes
}

#[tokio::test]
async fn every_api_route_waits_for_setup() {
    let (_, app) = fresh();
    let routes = declared_routes();
    assert!(
        routes.len() > 100,
        "the route list was read: {}",
        routes.len()
    );
    let open =
        |path: &str| path == "/auth/options" || path == "/privacy" || path.starts_with("/setup");
    let mut checked = 0;
    for (method, path) in &routes {
        if open(path) {
            continue;
        }
        let concrete = path
            .split('/')
            .map(|part| if part.starts_with('{') { "x" } else { part })
            .collect::<Vec<_>>()
            .join("/");
        let body = (method != "GET" && method != "DELETE").then(|| json!({}));
        let (status, answer) = send(
            &app,
            "198.51.100.1",
            method,
            &format!("/api/v1{concrete}"),
            body.as_ref(),
        )
        .await;
        assert_eq!(
            (status, answer["error"].as_str()),
            (StatusCode::SERVICE_UNAVAILABLE, Some("setupPending")),
            "{method} {path}"
        );
        checked += 1;
    }
    assert!(checked > 90, "{checked} routes checked");

    // the routes that stay open answer as themselves
    let (status, _) = send(&app, "198.51.100.1", "GET", "/api/v1/info", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(&app, "198.51.100.1", "GET", "/api/v1/privacy", None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn other_spellings_of_a_route_reach_nothing() {
    let (_, app) = fresh();
    for uri in [
        "//api/v1/me",
        "/api//v1/me",
        "/api/v1//me",
        "/API/v1/me",
        "/api/V1/me",
        "/api/v1/setup/../me",
        "/api/v1/setup/%2e%2e/me",
        "/api/v1/setup/..%2fme",
        "/api/v1/%6de",
        "/api/v1/me/",
        "/api/v1/me;x",
        "/api/v1/me%00",
        "/api/v1/setup%2f../me",
        "/./api/v1/me",
        "/health/../api/v1/me",
    ] {
        let (status, body) = send(&app, "198.51.100.1", "GET", uri, None).await;
        assert!(
            status == StatusCode::NOT_FOUND
                || status == StatusCode::SERVICE_UNAVAILABLE
                || status == StatusCode::BAD_REQUEST,
            "{uri}: {status} {body}"
        );
        assert_ne!(body["error"], "unauthorized", "{uri} reached a handler");
    }
}
