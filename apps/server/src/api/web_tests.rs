// SPDX-License-Identifier: AGPL-3.0-or-later

//! The web version: off by default, who may open it, its sign-in cookie
//! and the sign-in limits, through the full router.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::json;
use tower::ServiceExt;

use super::tests::{admin_on, call, server_with};
use crate::accounts::store;
use crate::config::Config;
use crate::routes::AppState;

/// A page folder with the admin page and a web build in it.
fn pages() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let web = dir.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::create_dir_all(web.join("fonts").join("inter")).unwrap();
    std::fs::write(
        dir.path().join("admin.html"),
        "<!doctype html><head><title>admin</title></head>",
    )
    .unwrap();
    std::fs::write(
        web.join("index.html"),
        "<!doctype html><title>notes</title>",
    )
    .unwrap();
    std::fs::write(web.join("assets").join("main-abc.js"), "run()").unwrap();
    std::fs::write(
        web.join("fonts").join("inter").join("latin-400.woff2"),
        "wOF2",
    )
    .unwrap();
    dir
}

async fn with_pages(dir: &tempfile::TempDir) -> (AppState, Router, String) {
    let mut config = Config::default();
    config.server.web_dir = Some(dir.path().to_path_buf());
    admin_on(server_with(config).await).await
}

async fn get(app: &Router, path: &str, cookie: Option<&str>) -> (StatusCode, HeaderMap, String) {
    let mut req = Request::get(path);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

/// Signs this browser in to the web version, returns its cookie as sent back.
async fn session(app: &Router, token: &str) -> (StatusCode, Option<String>) {
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/web/session")
                .header(
                    app_protocol::PROTOCOL_HEADER,
                    app_protocol::PROTOCOL_VERSION.to_string(),
                )
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().to_owned());
    (response.status(), cookie)
}

#[tokio::test]
async fn off_until_an_admin_turns_it_on() {
    let dir = pages();
    let (_state, app, admin) = with_pages(&dir).await;
    for path in [
        "/web",
        "/web/",
        "/web/assets/main-abc.js",
        "/web/index.html",
    ] {
        assert_eq!(
            get(&app, path, None).await.0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    let (status, view) = call(&app, "GET", "/admin/web", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["access"], "off");
    assert_eq!(view["files"], true);
    assert_eq!(view["defaults"]["attempts"], 5);
    assert_eq!(
        session(&app, &admin).await.0,
        StatusCode::NOT_FOUND,
        "no cookie while off"
    );
}

#[tokio::test]
async fn open_to_anyone_serves_the_build_and_nothing_beside_it() {
    let dir = pages();
    let (_state, app, admin) = with_pages(&dir).await;
    let (status, view) = call(
        &app,
        "PUT",
        "/admin/web",
        Some(&admin),
        Some(json!({ "access": "anyone" })),
    )
    .await;
    assert_eq!(
        (status, &view["access"]),
        (StatusCode::OK, &json!("anyone"))
    );

    let (status, headers, _) = get(&app, "/web", None).await;
    assert_eq!(
        (status, headers[header::LOCATION].to_str().unwrap()),
        (StatusCode::TEMPORARY_REDIRECT, "/web/")
    );
    let (status, _, body) = get(&app, "/web/", None).await;
    assert_eq!(
        (status, body.contains("<title>notes")),
        (StatusCode::OK, true)
    );
    let (status, headers, _) = get(&app, "/web/assets/main-abc.js", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("private")
    );
    assert!(
        headers[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("wasm-unsafe-eval")
    );
    assert_eq!(
        get(&app, "/web/fonts/inter/latin-400.woff2", None).await.0,
        StatusCode::OK
    );
    for outside in [
        "/web/..%2Fadmin.html",
        "/web/assets/..%2F..%2Fadmin.html",
        "/web/.hidden",
        "/web/assets/missing.js",
        "/web/a/b/c/d",
    ] {
        assert_eq!(
            get(&app, outside, None).await.0,
            StatusCode::NOT_FOUND,
            "{outside}"
        );
    }
}

#[tokio::test]
async fn behind_the_sign_in_nothing_loads_without_the_cookie() {
    let dir = pages();
    let (state, app, admin) = with_pages(&dir).await;
    call(
        &app,
        "PUT",
        "/admin/web",
        Some(&admin),
        Some(json!({ "access": "signedIn" })),
    )
    .await;

    // the sign-in page, and no file of the app
    let (status, _, body) = get(&app, "/web/", None).await;
    assert_eq!(
        (status, body.contains("<title>admin")),
        (StatusCode::OK, true)
    );
    assert!(!body.contains("notes"));
    for path in [
        "/web/assets/main-abc.js",
        "/web/index.html",
        "/web/fonts/inter/latin-400.woff2",
    ] {
        assert_eq!(
            get(&app, path, None).await.0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
        assert_eq!(
            get(&app, path, Some("web_session=wv_made-up")).await.0,
            StatusCode::NOT_FOUND,
            "{path} with a made up cookie"
        );
    }

    let (status, cookie) = session(&app, &admin).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cookie = cookie.unwrap();
    assert!(cookie.starts_with("web_session=wv_"), "{cookie}");
    for part in ["Path=/web/", "HttpOnly", "SameSite=Strict"] {
        assert!(cookie.contains(part), "{cookie}");
    }
    let pair = cookie.split(';').next().unwrap();
    let (status, _, body) = get(&app, "/web/", Some(pair)).await;
    assert_eq!(
        (status, body.contains("<title>notes")),
        (StatusCode::OK, true)
    );
    assert_eq!(
        get(&app, "/web/assets/main-abc.js", Some(pair)).await.0,
        StatusCode::OK
    );

    // signing the device out ends it
    let token = pair.trim_start_matches("web_session=").to_owned();
    let now = state.clock.now();
    let device: String = state
        .db
        .call(move |c| {
            Ok(c.query_row(
                "SELECT device_id FROM web_sessions WHERE hash = ?1",
                [crate::accounts::token::hash(&token).as_slice()],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    state
        .db
        .call(move |c| store::revoke_device(c, &device, now))
        .await
        .unwrap();
    assert_eq!(
        get(&app, "/web/assets/main-abc.js", Some(pair)).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn turning_it_off_ends_every_session() {
    let dir = pages();
    let (state, app, admin) = with_pages(&dir).await;
    call(
        &app,
        "PUT",
        "/admin/web",
        Some(&admin),
        Some(json!({ "access": "signedIn" })),
    )
    .await;
    let (_, cookie) = session(&app, &admin).await;
    let pair = cookie.unwrap().split(';').next().unwrap().to_owned();
    call(
        &app,
        "PUT",
        "/admin/web",
        Some(&admin),
        Some(json!({ "access": "off" })),
    )
    .await;
    let left: i64 = state
        .db
        .call(|c| Ok(c.query_row("SELECT count(*) FROM web_sessions", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(left, 0);
    call(
        &app,
        "PUT",
        "/admin/web",
        Some(&admin),
        Some(json!({ "access": "signedIn" })),
    )
    .await;
    assert_eq!(
        get(&app, "/web/assets/main-abc.js", Some(&pair)).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn it_cannot_be_turned_on_without_its_files() {
    let (_state, app, admin) = admin_on(server_with(Config::default()).await).await;
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/web",
        Some(&admin),
        Some(json!({ "access": "signedIn" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, view) = call(&app, "GET", "/admin/web", Some(&admin), None).await;
    assert_eq!(
        (&view["access"], &view["files"]),
        (&json!("off"), &json!(false))
    );
}

#[tokio::test]
async fn sign_in_limits_set_on_the_admin_page_count() {
    let dir = pages();
    let (state, app, admin) = with_pages(&dir).await;
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/web/limits",
        Some(&admin),
        Some(json!({ "firstWait": 30, "longestWait": 10 })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the longest wait is the longest"
    );
    let (status, view) = call(
        &app,
        "PUT",
        "/admin/web/limits",
        Some(&admin),
        Some(json!({ "attempts": 2, "perMinute": 30 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        (&view["limits"]["attempts"], &view["limits"]["firstWait"]),
        (&json!(2), &json!(null))
    );

    // the second wrong password now locks the account
    let auth = state.config.auth.clone();
    let locks = state
        .db
        .call(move |c| {
            let id: String =
                c.query_row("SELECT id FROM users WHERE username = 'admin'", [], |r| {
                    r.get(0)
                })?;
            Ok((
                store::record_failure(c, &id, 0, &auth)?,
                store::record_failure(c, &id, 0, &auth)?,
            ))
        })
        .await
        .unwrap();
    assert!(locks.0.is_none() && locks.1.is_some(), "{locks:?}");

    // back to the config file's values
    let (_, view) = call(
        &app,
        "PUT",
        "/admin/web/limits",
        Some(&admin),
        Some(json!({})),
    )
    .await;
    assert_eq!(view["limits"]["attempts"], json!(null));
}
