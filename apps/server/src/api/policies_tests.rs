// SPDX-License-Identifier: AGPL-3.0-or-later

//! The files policy set by the admin, through the full router: where
//! values come from, what the account gets, refusals, and the storage
//! overview.

use axum::Router;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::files_tests::{content, sha, upload};
use super::tests::{call, person, token, with_admin};

const MB: u64 = 1024 * 1024;

async fn put(app: &Router, admin: &str, path: &str, body: Value) -> (StatusCode, Value) {
    call(app, "PUT", path, Some(admin), Some(body)).await
}

async fn mine(app: &Router, session: &str) -> Value {
    let (status, info) = call(app, "GET", "/sync/files/policy", Some(session), None).await;
    assert_eq!(status, StatusCode::OK, "{info}");
    info["policy"].clone()
}

#[tokio::test]
async fn the_instance_policy_then_the_config() {
    let (_, app, admin) = with_admin().await;
    // the config file's defaults until anything is set
    let (_, view) = call(&app, "GET", "/admin/policy", Some(&admin), None).await;
    assert_eq!(view["settings"], json!({}));
    assert_eq!(view["sources"]["maxFileBytes"]["level"], "config");
    assert_eq!(mine(&app, &admin).await["maxFileBytes"], 250 * MB);

    let (status, view) = put(
        &app,
        &admin,
        "/admin/policy",
        json!({ "maxFileMb": 1, "quotaMb": 500, "blockedTypes": [".exe"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["effective"]["maxFileBytes"], MB);
    assert_eq!(view["sources"]["quotaBytes"]["level"], "instance");
    assert_eq!(view["sources"]["imageQuality"]["level"], "config");
    assert_eq!(mine(&app, &admin).await["quotaBytes"], 500 * MB);
    // and is enforced
    let big = content(2 * 1024 * 1024, 1);
    let (status, _) = upload(&app, &admin, None, &sha(&big), &big, 8 * 64 * 1024).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    // cleared, the config again
    put(&app, &admin, "/admin/policy", json!({})).await;
    assert_eq!(mine(&app, &admin).await["maxFileBytes"], 250 * MB);
}

#[tokio::test]
async fn bad_settings_and_other_accounts_are_refused() {
    let (state, app, admin) = with_admin().await;
    let anna = person(&state, "anna").await;
    for bad in [
        json!({ "imageQuality": 0 }),
        json!({ "blockedTypes": ["exe"] }),
        json!({ "allowedTypes": [".p df"] }),
    ] {
        let (status, _) = put(&app, &admin, "/admin/policy", bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _) = put(&app, token(&anna), "/admin/policy", json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, "GET", "/admin/storage", Some(token(&anna)), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // the change is in the security log
    put(&app, &admin, "/admin/policy", json!({ "quotaMb": 10 })).await;
    let (_, log) = call(
        &app,
        "GET",
        "/admin/audit?event=policy_changed",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(log["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_storage_overview_counts_what_one_copy_saves() {
    let (state, app, admin) = with_admin().await;
    let anna = person(&state, "anna").await;
    let shared = content(3 * 64 * 1024, 2);
    let own = content(64 * 1024, 3);
    upload(&app, token(&anna), None, &sha(&shared), &shared, 64 * 1024).await;
    upload(&app, token(&anna), None, &sha(&own), &own, 64 * 1024).await;
    upload(&app, &admin, None, &sha(&shared), &shared, 64 * 1024).await;
    let (status, overview) = call(&app, "GET", "/admin/storage", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    assert_eq!(overview["files"], 2);
    assert_eq!(overview["uniqueBytes"], 4 * 64 * 1024);
    assert_eq!(overview["referencedBytes"], 7 * 64 * 1024);
    assert_eq!(
        overview["storedBytes"],
        app_e2ee::sealed_size(3 * 64 * 1024) + app_e2ee::sealed_size(64 * 1024)
    );
    assert!(overview.get("people").is_none());
}
