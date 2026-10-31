// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin and group endpoints through the full router.

use axum::http::StatusCode;
use serde_json::json;

use super::tests::{call, login, with_admin};

/// Creates an account through the admin api, returns its id.
async fn add_user(app: &axum::Router, admin: &str, username: &str, role: &str) -> String {
    let body =
        json!({ "username": username, "password": format!("{username} password"), "role": role });
    let (status, user) = call(app, "POST", "/admin/users", Some(admin), Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{user}");
    user["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_last_active_admin_cannot_be_removed_demoted_or_turned_off() {
    let (_, app, admin) = with_admin().await;
    let (_, me) = call(&app, "GET", "/me", Some(&admin), None).await;
    let me_path = format!("/admin/users/{}", me["id"].as_str().unwrap());
    for change in [json!({ "role": "user" }), json!({ "disabled": true })] {
        let (status, body) = call(&app, "PATCH", &me_path, Some(&admin), Some(change)).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (StatusCode::CONFLICT, Some("lastAdmin"))
        );
    }
    let (status, _) = call(&app, "DELETE", &me_path, Some(&admin), None).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // with a second admin the first can step down
    add_user(&app, &admin, "second", "admin").await;
    let (status, body) = call(
        &app,
        "PATCH",
        &me_path,
        Some(&admin),
        Some(json!({ "role": "user" })),
    )
    .await;
    assert_eq!(
        (status, body["role"].as_str()),
        (StatusCode::OK, Some("user"))
    );
    let (status, _) = call(&app, "GET", "/admin/overview", Some(&admin), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "no longer an admin");
}

#[tokio::test]
async fn turning_an_account_off_signs_it_out_and_refuses_sign_in() {
    let (_, app, admin) = with_admin().await;
    let id = add_user(&app, &admin, "anna", "user").await;
    let (_, session) = login(&app, "anna", "anna password").await;
    let anna = session["accessToken"].as_str().unwrap().to_owned();

    let path = format!("/admin/users/{id}");
    let (status, user) = call(
        &app,
        "PATCH",
        &path,
        Some(&admin),
        Some(json!({ "disabled": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(user["disabledAt"].is_i64());
    assert_eq!(
        call(&app, "GET", "/me", Some(&anna), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, body) = login(&app, "anna", "anna password").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("accountDisabled"))
    );

    call(
        &app,
        "PATCH",
        &path,
        Some(&admin),
        Some(json!({ "disabled": false, "displayName": "Anna B." })),
    )
    .await;
    let (status, session) = login(&app, "anna", "anna password").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["user"]["displayName"], "Anna B.");
}

#[tokio::test]
async fn a_password_reset_signs_every_device_out() {
    let (_, app, admin) = with_admin().await;
    let id = add_user(&app, &admin, "anna", "user").await;
    let (_, session) = login(&app, "anna", "anna password").await;
    let path = format!("/admin/users/{id}/password");
    let (status, body) = call(
        &app,
        "POST",
        &path,
        Some(&admin),
        Some(json!({ "password": "short" })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("passwordWeak"))
    );
    let (status, _) = call(
        &app,
        "POST",
        &path,
        Some(&admin),
        Some(json!({ "password": "fresh password" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(&app, "GET", "/me", session["accessToken"].as_str(), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, "anna", "anna password").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, "anna", "fresh password").await.0,
        StatusCode::OK
    );

    let (_, detail) = call(
        &app,
        "GET",
        &format!("/admin/users/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(
        detail["deviceList"].as_array().unwrap().len(),
        1,
        "the new sign-in"
    );
    let (status, _) = call(
        &app,
        "POST",
        &format!("/admin/users/{id}/signout"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, detail) = call(
        &app,
        "GET",
        &format!("/admin/users/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(detail["user"]["devices"], 0);
}

#[tokio::test]
async fn users_search_pages_and_deletion() {
    let (_, app, admin) = with_admin().await;
    for name in ["anna", "annabel", "bob"] {
        add_user(&app, &admin, name, "user").await;
    }
    let (_, page) = call(
        &app,
        "GET",
        "/admin/users?query=ann&limit=1",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(page["total"], 2);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["username"], "anna");
    let (_, second) = call(
        &app,
        "GET",
        "/admin/users?query=ann&limit=1&offset=1",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(second["items"][0]["username"], "annabel");

    let bob = page_id(&app, &admin, "bob").await;
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/admin/users/{bob}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        login(&app, "bob", "bob password").await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, overview) = call(&app, "GET", "/admin/overview", Some(&admin), None).await;
    assert_eq!(
        (overview["users"].as_u64(), overview["admins"].as_u64()),
        (Some(3), Some(1))
    );
}

async fn page_id(app: &axum::Router, admin: &str, username: &str) -> String {
    let (_, page) = call(
        app,
        "GET",
        &format!("/admin/users?query={username}"),
        Some(admin),
        None,
    )
    .await;
    page["items"][0]["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn groups_and_their_members() {
    let (_, app, admin) = with_admin().await;
    let anna = add_user(&app, &admin, "anna", "user").await;
    let (status, group) = call(
        &app,
        "POST",
        "/admin/groups",
        Some(&admin),
        Some(json!({ "name": " Team " })),
    )
    .await;
    assert_eq!(
        (status, group["name"].as_str()),
        (StatusCode::CREATED, Some("Team"))
    );
    let (status, body) = call(
        &app,
        "POST",
        "/admin/groups",
        Some(&admin),
        Some(json!({ "name": "team" })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::CONFLICT, Some("groupNameTaken"))
    );

    let gid = group["id"].as_str().unwrap();
    let member = format!("/admin/groups/{gid}/members/{anna}");
    assert_eq!(
        call(&app, "PUT", &member, Some(&admin), None).await.0,
        StatusCode::NO_CONTENT
    );
    let (_, session) = login(&app, "anna", "anna password").await;
    let (_, mine) = call(
        &app,
        "GET",
        "/me/groups",
        session["accessToken"].as_str(),
        None,
    )
    .await;
    assert_eq!(mine[0]["name"], "Team", "people see their groups");
    let (_, user) = call(
        &app,
        "GET",
        &format!("/admin/users/{anna}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(user["user"]["groups"], json!(["Team"]));

    let (_, renamed) = call(
        &app,
        "PATCH",
        &format!("/admin/groups/{gid}"),
        Some(&admin),
        Some(json!({ "name": "Crew" })),
    )
    .await;
    assert_eq!(renamed["members"], 1);
    let (_, detail) = call(
        &app,
        "GET",
        &format!("/admin/groups/{gid}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(detail["memberList"][0]["username"], "anna");
    assert_eq!(
        call(&app, "DELETE", &member, Some(&admin), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "DELETE", &member, Some(&admin), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/admin/groups/{gid}"),
            Some(&admin),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, groups) = call(&app, "GET", "/admin/groups", Some(&admin), None).await;
    assert_eq!(groups, json!([]));
}

#[tokio::test]
async fn the_security_log_filters_and_pages() {
    let (_, app, admin) = with_admin().await;
    let anna = add_user(&app, &admin, "anna", "user").await;
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
    let events: Vec<&str> = hers["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event"].as_str().unwrap())
        .collect();
    assert!(
        events.contains(&"user_created") && events.contains(&"sign_in_failed"),
        "{events:?}"
    );
    let created = hers["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["event"] == "user_created")
        .unwrap();
    assert_eq!(created["detail"], "by admin", "the acting admin is kept");
}

#[tokio::test]
async fn people_who_are_not_admins_get_nothing() {
    let (_, app, admin) = with_admin().await;
    add_user(&app, &admin, "anna", "user").await;
    let (_, session) = login(&app, "anna", "anna password").await;
    let anna = session["accessToken"].as_str().unwrap();
    for path in [
        "/admin/overview",
        "/admin/users",
        "/admin/groups",
        "/admin/audit",
    ] {
        assert_eq!(
            call(&app, "GET", path, Some(anna), None).await.0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
}
