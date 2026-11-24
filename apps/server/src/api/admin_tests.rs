// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin endpoints through the full router.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use super::tests::{admin_on, call, login, person, server_with, token, user_id, with_admin};
use crate::accounts::store;
use crate::config::Config;

#[tokio::test]
async fn the_overview_counts_what_one_person_has() {
    let (_, app, admin) = with_admin().await;
    let (status, overview) = call(&app, "GET", "/admin/overview", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    assert_eq!(overview["devices"], 1);
    assert_eq!(overview["coediting"], true);
    assert!(overview.get("users").is_none());
}

#[tokio::test]
async fn the_security_log_filters_and_pages() {
    let (state, app, admin) = with_admin().await;
    let anna = person(&state, "anna").await;
    let anna = user_id(&anna).to_owned();
    for _ in 0..3 {
        login(&app, "anna", "wrong password").await;
    }
    let (_, failed) = call(
        &app,
        "GET",
        "/admin/audit?event=sign_in_failed&limit=2",
        Some(&admin),
        None,
    )
    .await;
    let items = failed["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["username"], "anna");
    let before = failed["nextBefore"].as_i64().unwrap();
    let (_, older) = call(
        &app,
        "GET",
        &format!("/admin/audit?event=sign_in_failed&limit=2&before={before}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(older["items"].as_array().unwrap().len(), 1);
    assert!(older["nextBefore"].is_null(), "the end");

    let (_, hers) = call(
        &app,
        "GET",
        &format!("/admin/audit?userId={anna}"),
        Some(&admin),
        None,
    )
    .await;
    assert!(
        hers["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["event"] == "sign_in_failed"),
        "{hers}"
    );
}

/// Another account a server for several people may still hold.
#[tokio::test]
async fn people_who_are_not_admins_get_nothing() {
    let (state, app, _) = with_admin().await;
    let anna = person(&state, "anna").await;
    for path in [
        "/admin/overview",
        "/admin/audit",
        "/admin/policy",
        "/admin/backups",
    ] {
        assert_eq!(
            call(&app, "GET", path, Some(token(&anna)), None).await.0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
}

#[tokio::test]
async fn sign_in_limits_set_on_the_admin_page_count() {
    let (state, app, admin) = with_admin().await;
    let (status, view) = call(&app, "GET", "/admin/signin-limits", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["defaults"]["attempts"], 5);
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/signin-limits",
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
        "/admin/signin-limits",
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
            let me = store::Subject::Account(&id);
            let ip = std::net::IpAddr::from([203, 0, 113, 1]);
            Ok((
                store::record_failure(c, me, ip, 0, &auth)?,
                store::record_failure(c, me, ip, 0, &auth)?,
            ))
        })
        .await
        .unwrap();
    assert!(locks.0.is_none() && locks.1.is_some(), "{locks:?}");

    // back to the config file's values
    let (_, view) = call(
        &app,
        "PUT",
        "/admin/signin-limits",
        Some(&admin),
        Some(json!({})),
    )
    .await;
    assert_eq!(view["limits"]["attempts"], json!(null));
}

#[tokio::test]
async fn the_web_version_is_not_served_here_even_with_its_build_beside_the_pages() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("web")).unwrap();
    std::fs::write(dir.path().join("web").join("index.html"), "notes").unwrap();
    std::fs::write(dir.path().join("admin.html"), "<head></head>").unwrap();
    let mut config = Config::default();
    config.server.web_dir = Some(dir.path().to_path_buf());
    let (_, app, admin) = admin_on(server_with(config).await).await;
    for path in ["/app", "/app/", "/web", "/web/", "/web/index.html"] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    let (status, _) = call(&app, "GET", "/admin/web", Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
