// SPDX-License-Identifier: AGPL-3.0-or-later

//! Files policies set by the admin, through the full router: levels and
//! where values come from, what people get, refusals, and the storage
//! overview.

use axum::Router;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::files_tests::{content, sha, upload};
use super::sharing_tests::{open_registration, standard, token, user_id};
use super::tests::{call, with_admin};

const MB: u64 = 1024 * 1024;

async fn put(app: &Router, admin: &str, path: &str, body: Value) -> (StatusCode, Value) {
    call(app, "PUT", path, Some(admin), Some(body)).await
}

async fn mine(app: &Router, session: &Value) -> Value {
    let (status, info) = call(app, "GET", "/sync/files/policy", Some(token(session)), None).await;
    assert_eq!(status, StatusCode::OK, "{info}");
    info["policy"].clone()
}

async fn group(app: &Router, admin: &str, name: &str, members: &[&Value]) -> String {
    let (status, group) = call(
        app,
        "POST",
        "/admin/groups",
        Some(admin),
        Some(json!({ "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{group}");
    let id = group["id"].as_str().unwrap().to_owned();
    for member in members {
        let path = format!("/admin/groups/{id}/members/{}", user_id(member));
        let (status, _) = call(app, "PUT", &path, Some(admin), None).await;
        assert!(status.is_success());
    }
    id
}

#[tokio::test]
async fn people_get_their_own_then_their_groups_then_the_instance() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let anna = standard(&app, "anna").await;
    let ben = standard(&app, "ben").await;
    // the config file's defaults until anything is set
    let (_, view) = call(&app, "GET", "/admin/policy", Some(&admin), None).await;
    assert_eq!(view["settings"], json!({}));
    assert_eq!(view["sources"]["maxFileBytes"]["level"], "config");
    assert_eq!(mine(&app, &anna).await["maxFileBytes"], 100 * MB);

    let (status, view) = put(
        &app,
        &admin,
        "/admin/policy",
        json!({ "maxFileMb": 20, "quotaMb": 500, "blockedTypes": [".exe"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["effective"]["maxFileBytes"], 20 * MB);
    assert_eq!(view["sources"]["quotaBytes"]["level"], "instance");
    assert_eq!(mine(&app, &ben).await["quotaBytes"], 500 * MB);

    let family = group(&app, &admin, "Family", &[&anna]).await;
    let (status, view) = put(
        &app,
        &admin,
        &format!("/admin/groups/{family}/policy"),
        json!({ "quotaMb": 0, "thumbnails": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["effective"]["quotaBytes"], 0);
    assert_eq!(
        view["effective"]["maxFileBytes"],
        20 * MB,
        "from the instance"
    );
    let anna_now = mine(&app, &anna).await;
    assert_eq!(anna_now["quotaBytes"], 0);
    assert_eq!(anna_now["thumbnails"], false);
    assert_eq!(mine(&app, &ben).await["quotaBytes"], 500 * MB, "not in it");

    // a second group: the most generous of the two
    group(&app, &admin, "Work", &[&anna]).await;
    let work = {
        let (_, groups) = call(&app, "GET", "/admin/groups", Some(&admin), None).await;
        groups
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["name"] == "Work")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    put(
        &app,
        &admin,
        &format!("/admin/groups/{work}/policy"),
        json!({ "thumbnails": true, "maxFileMb": 5 }),
    )
    .await;
    let path = format!("/admin/users/{}/policy", user_id(&anna));
    let (_, view) = call(&app, "GET", &path, Some(&admin), None).await;
    assert_eq!(view["effective"]["thumbnails"], true);
    assert_eq!(
        view["sources"]["thumbnails"]["groups"],
        json!(["Family", "Work"])
    );
    assert_eq!(view["effective"]["maxFileBytes"], 5 * MB);
    assert_eq!(view["sources"]["maxFileBytes"]["groups"], json!(["Work"]));

    // the person's own value wins
    let (status, view) = put(&app, &admin, &path, json!({ "maxFileMb": 1 })).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["sources"]["maxFileBytes"]["level"], "user");
    assert_eq!(mine(&app, &anna).await["maxFileBytes"], MB);
    // and is enforced
    let big = content(2 * 1024 * 1024, 1);
    let (status, _) = upload(&app, token(&anna), None, &sha(&big), &big, 8 * 64 * 1024).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    // cleared, it inherits again
    put(&app, &admin, &path, json!({})).await;
    assert_eq!(mine(&app, &anna).await["maxFileBytes"], 5 * MB);

    // a deleted group takes its settings
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/admin/groups/{work}"),
        Some(&admin),
        None,
    )
    .await;
    assert!(status.is_success());
    assert_eq!(mine(&app, &anna).await["maxFileBytes"], 20 * MB);
}

#[tokio::test]
async fn bad_settings_strangers_and_unknown_targets_are_refused() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let anna = standard(&app, "anna").await;
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
    let (status, _) = put(&app, &admin, "/admin/groups/nope/policy", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, "GET", "/admin/users/nope/policy", Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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
async fn the_storage_overview_counts_people_and_what_one_copy_saves() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let anna = standard(&app, "anna").await;
    let shared = content(3 * 64 * 1024, 2);
    let own = content(64 * 1024, 3);
    upload(&app, token(&anna), None, &sha(&shared), &shared, 64 * 1024).await;
    upload(&app, token(&anna), None, &sha(&own), &own, 64 * 1024).await;
    upload(&app, &admin, None, &sha(&shared), &shared, 64 * 1024).await;
    put(&app, &admin, "/admin/policy", json!({ "quotaMb": 50 })).await;
    let (status, overview) = call(&app, "GET", "/admin/storage", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    assert_eq!(overview["files"], 2);
    assert_eq!(overview["uniqueBytes"], 4 * 64 * 1024);
    assert_eq!(overview["referencedBytes"], 7 * 64 * 1024);
    assert_eq!(
        overview["storedBytes"],
        app_e2ee::sealed_size(3 * 64 * 1024) + app_e2ee::sealed_size(64 * 1024)
    );
    let people = overview["people"]["items"].as_array().unwrap();
    assert_eq!(overview["people"]["total"], 2);
    assert_eq!(people[0]["username"], "anna", "most used first");
    assert_eq!(people[0]["fileBytes"], 4 * 64 * 1024);
    assert_eq!(people[0]["files"], 2);
    assert_eq!(people[0]["largestBytes"], 3 * 64 * 1024);
    assert_eq!(people[0]["quotaBytes"], 50 * MB);
    assert_eq!(people[1]["fileBytes"], 3 * 64 * 1024);
    let (_, page) = call(
        &app,
        "GET",
        "/admin/storage?offset=1&limit=1",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(page["people"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["people"]["items"][0]["username"], "admin");
}
