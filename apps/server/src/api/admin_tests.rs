// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin endpoints through the full router.

use axum::http::StatusCode;

use super::tests::{call, login, person, token, user_id, with_admin};

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
