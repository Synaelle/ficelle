// SPDX-License-Identifier: AGPL-3.0-or-later

//! The instance's backups through the full router: set up by an admin,
//! run, listed, checked, test restored; a deleted account brought back with
//! its shared space, its changes and its file; a whole instance restored
//! into an empty folder.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::files_tests::{content, sha, upload};
use super::sharing_tests::{open_registration, share, standard, token, user_id};
use super::tests::{admin_on, call, login, server_with};
use crate::config::{Config, StorageConfig};

async fn set_up(
    app: &axum::Router,
    admin: &str,
    folder: &std::path::Path,
    create: bool,
    password: &str,
) -> (StatusCode, Value) {
    call(
        app,
        "PUT",
        "/admin/backups",
        Some(admin),
        Some(json!({
            "target": { "kind": "folder", "path": folder.to_string_lossy() },
            "password": password,
            "create": create,
            "offsetSeconds": 3600,
        })),
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
#[expect(clippy::too_many_lines, reason = "one story, read top to bottom")]
async fn an_instance_is_backed_up_and_a_deleted_account_comes_back() {
    let data = tempfile::tempdir().unwrap();
    let place = tempfile::tempdir().unwrap();
    let config = Config {
        storage: StorageConfig {
            data_dir: data.path().to_owned(),
            ..StorageConfig::default()
        },
        ..Config::default()
    };
    let (state, app, admin) = admin_on(server_with(config).await).await;
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
    let kim = standard(&app, "kim").await;
    let space = share(
        &app,
        &alex,
        json!([{ "userId": user_id(&kim), "role": "edit" }]),
    )
    .await;
    let push = json!({ "changes": [
        { "id": "c1", "doc": "space", "data": "AAE=" },
        { "id": "c2", "doc": "space", "data": "AAI=" },
    ] });
    let (status, _) = call(
        &app,
        "POST",
        &format!("/sync/push?space={space}"),
        Some(token(&alex)),
        Some(push),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let bytes = content(300_000, 7);
    let (status, body) = upload(
        &app,
        token(&alex),
        Some(&space),
        &sha(&bytes),
        &bytes,
        64 * 1024,
    )
    .await;
    assert!(status.is_success(), "{body}");

    // only an admin sets backups up
    let (status, _) = set_up(&app, token(&kim), place.path(), true, "pw").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, view) = set_up(&app, &admin, place.path(), true, "backup pw").await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["config"]["schedule"], "daily");
    assert_eq!(view["config"]["policy"]["last"], 10);
    // a second repository at the same place is refused, and a wrong password
    let (status, body) = set_up(&app, &admin, place.path(), true, "other").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    let (status, body) = set_up(&app, &admin, place.path(), false, "wrong").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(
        body["message"].as_str().unwrap().contains("wrong password"),
        "{body}"
    );
    let (status, _) = set_up(&app, &admin, place.path(), false, "backup pw").await;
    assert_eq!(status, StatusCode::OK);

    // a run, listed, checked, test restored
    // the feedback database goes along
    state
        .feedback
        .call(|c| {
            crate::feedback::block(c, 0, crate::feedback::BlockKind::Word, "casino", "", None)
        })
        .await
        .unwrap();
    let (status, view) = call(&app, "POST", "/admin/backups/run", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert!(view["status"]["lastSuccessAt"].is_i64());
    assert_eq!(view["status"]["snapshots"], 1);
    assert_eq!(view["status"]["lastReport"]["users"], 3);
    assert_eq!(view["status"]["lastReport"]["files"], 1);
    let (_, again) = call(&app, "POST", "/admin/backups/run", Some(&admin), None).await;
    // the database is read again, the file is taken from the first snapshot
    let read = |v: &Value| {
        v["status"]["lastReport"]["stats"]["readBytes"]
            .as_u64()
            .unwrap()
    };
    assert!(
        read(&again) + 250_000 < read(&view),
        "the file is not read again: {again}"
    );
    let (_, list) = call(&app, "GET", "/admin/backups/snapshots", Some(&admin), None).await;
    assert_eq!(list.as_array().unwrap().len(), 2, "{list}");
    let first = list[1]["id"].as_str().unwrap().to_owned();
    assert_eq!(list[0]["counts"]["users"], 3);
    let (_, check) = call(
        &app,
        "POST",
        "/admin/backups/check",
        Some(&admin),
        Some(json!({ "full": true })),
    )
    .await;
    assert_eq!(check["problems"], json!([]), "{check}");
    let (_, tried) = call(
        &app,
        "POST",
        "/admin/backups/test-restore",
        Some(&admin),
        Some(json!({ "snapshot": first })),
    )
    .await;
    assert_eq!(tried["problems"], json!([]), "{tried}");
    assert_eq!(tried["users"], 3);
    assert_eq!(tried["files"], 1);
    let left: Vec<_> = std::fs::read_dir(data.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("backup-"))
        .collect();
    assert!(left.is_empty(), "scratch folders are removed");

    // alex is deleted: their account, their space, its changes
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/admin/users/{}", user_id(&alex)),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        "GET",
        &format!("/sync/changes?space={space}"),
        Some(token(&kim)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // their file goes too (as the sweep would)
    state
        .db
        .call(|c| {
            c.execute(
                "DELETE FROM files WHERE object NOT IN (SELECT object FROM file_refs)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    // and comes back, with the space, its changes, kim's membership, the file
    let (status, back) = call(
        &app,
        "POST",
        "/admin/backups/restore-person",
        Some(&admin),
        Some(json!({ "snapshot": first, "username": "alex" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{back}");
    assert_eq!(back["account"], true);
    assert_eq!(back["spaces"], 1);
    assert_eq!(back["changes"], 2);
    let (status, _) = login(&app, "alex", "alex password").await;
    assert_eq!(status, StatusCode::OK, "the password is theirs again");
    let (status, changes) = call(
        &app,
        "GET",
        &format!("/sync/changes?space={space}"),
        Some(token(&kim)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "kim is a member again");
    assert_eq!(
        changes["changes"].as_array().map(Vec::len),
        Some(2),
        "{changes}"
    );
    let objects: i64 = state
        .db
        .call(|c| Ok(c.query_row("SELECT count(*) FROM files", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(objects, 1, "the file's row is back");
    // twice changes nothing
    let (_, twice) = call(
        &app,
        "POST",
        "/admin/backups/restore-person",
        Some(&admin),
        Some(json!({ "snapshot": first, "username": "alex" })),
    )
    .await;
    assert_eq!(twice["account"], false);
    assert_eq!(twice["spaces"], 0);

    // someone else took the name: refused, nothing changed
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/admin/users/{}", user_id(&alex)),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    standard(&app, "alex").await;
    let (status, body) = call(
        &app,
        "POST",
        "/admin/backups/restore-person",
        Some(&admin),
        Some(json!({ "snapshot": first, "username": "alex" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("another account"),
        "{body}"
    );

    // a whole instance into an empty folder, never over one in use
    let master = state.master.clone();
    let config = state
        .db
        .call(move |c| crate::backups::config(c, &master))
        .await
        .unwrap()
        .unwrap();
    let into = tempfile::tempdir().unwrap();
    let repo = crate::backups::open(&config, false, 0).unwrap();
    // a folder in use: the test server keeps its database in memory
    std::fs::write(data.path().join("server.db"), b"in use").unwrap();
    assert!(crate::backups::instance::restore(&repo, &first, data.path()).is_err());
    let restored =
        crate::backups::instance::restore(&repo, &first, &into.path().join("data")).unwrap();
    assert_eq!(restored.counts["users"], 3);
    let db = crate::db::Db::open(&into.path().join("data")).unwrap();
    let users: i64 = db
        .call(|c| Ok(c.query_row("SELECT count(*) FROM users", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(users, 3);
    let feedback = crate::db::Db::open_feedback(&into.path().join("data")).unwrap();
    let words: i64 = feedback
        .call(|c| Ok(c.query_row("SELECT count(*) FROM blocks", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(words, 1, "the feedback database came back");
    let disk = crate::files::LocalDisk::new(into.path().join("data").join("files"));
    // standard files are sealed at rest: what comes back is what was kept
    let whole = |storage: &dyn crate::files::Storage, object: &str| {
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut storage.read(object, 0).unwrap(), &mut out).unwrap();
        out
    };
    let object: String = state
        .db
        .call(|c| Ok(c.query_row("SELECT object FROM files", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(
        whole(&disk, &object),
        whole(state.files.as_ref(), &object),
        "the file comes back as kept"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_that_cannot_be_made_says_why_and_is_logged() {
    let data = tempfile::tempdir().unwrap();
    let place = tempfile::tempdir().unwrap();
    let config = Config {
        storage: StorageConfig {
            data_dir: data.path().to_owned(),
            ..StorageConfig::default()
        },
        ..Config::default()
    };
    let (_, app, admin) = admin_on(server_with(config).await).await;
    let target = place.path().join("repo");
    let (status, _) = set_up(&app, &admin, &target, true, "pw").await;
    assert_eq!(status, StatusCode::OK);
    // the place goes away
    std::fs::remove_dir_all(&target).unwrap();
    let (status, body) = call(&app, "POST", "/admin/backups/run", Some(&admin), None).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("no backup at this place"),
        "{body}"
    );
    let (_, view) = call(&app, "GET", "/admin/backups", Some(&admin), None).await;
    assert!(
        view["status"]["lastError"]
            .as_str()
            .unwrap()
            .contains("no backup"),
        "{view}"
    );
    assert!(view["status"]["lastSuccessAt"].is_null());
    let (_, log) = call(&app, "GET", "/admin/audit", Some(&admin), None).await;
    assert!(log.to_string().contains("backup_failed"), "{log}");
    // stopping keeps the status, clears the settings
    let (_, stopped) = call(&app, "DELETE", "/admin/backups", Some(&admin), None).await;
    assert!(stopped["config"].is_null());
}

/// Costs at scale, run by hand in release:
/// `cargo test -p server --release instance_backup_costs -- --ignored --nocapture`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a measurement, run by hand"]
async fn instance_backup_costs() {
    use std::time::Instant;
    let data = tempfile::tempdir().unwrap();
    let place = tempfile::tempdir().unwrap();
    let config = Config {
        storage: StorageConfig {
            data_dir: data.path().to_owned(),
            ..StorageConfig::default()
        },
        auth: crate::config::AuthConfig {
            rate_limit_per_minute: 1_000_000,
            ..crate::config::AuthConfig::default()
        },
        ..Config::default()
    };
    let (_, app, admin) = admin_on(server_with(config).await).await;
    open_registration(&app, &admin).await;
    let mut people = Vec::new();
    for i in 0..200 {
        let person = standard(&app, &format!("person{i}")).await;
        let space = share(&app, &person, json!([])).await;
        let changes: Vec<Value> = (0..50)
            .map(|c| json!({ "id": format!("c{c}"), "doc": "space", "data": "AAECAwQFBgcICQ==" }))
            .collect();
        call(
            &app,
            "POST",
            &format!("/sync/push?space={space}"),
            Some(token(&person)),
            Some(json!({ "changes": changes })),
        )
        .await;
        if i < 100 {
            for f in 0..3 {
                let bytes = content(100 * 1024, u8::try_from((i * 3 + f) % 250).unwrap());
                upload(
                    &app,
                    token(&person),
                    Some(&space),
                    &sha(&bytes),
                    &bytes,
                    64 * 1024,
                )
                .await;
            }
        }
        people.push(person);
    }
    set_up(&app, &admin, place.path(), true, "pw").await;
    let started = Instant::now();
    let (_, first) = call(&app, "POST", "/admin/backups/run", Some(&admin), None).await;
    println!(
        "first backup: {:.2} s, {}",
        started.elapsed().as_secs_f64(),
        first["status"]["lastReport"]["stats"]
    );
    let started = Instant::now();
    let (_, again) = call(&app, "POST", "/admin/backups/run", Some(&admin), None).await;
    println!(
        "again: {:.2} s, {}",
        started.elapsed().as_secs_f64(),
        again["status"]["lastReport"]["stats"]
    );
    let (_, list) = call(&app, "GET", "/admin/backups/snapshots", Some(&admin), None).await;
    let id = list[0]["id"].as_str().unwrap().to_owned();
    let started = Instant::now();
    let (_, tried) = call(
        &app,
        "POST",
        "/admin/backups/test-restore",
        Some(&admin),
        Some(json!({ "snapshot": id })),
    )
    .await;
    println!(
        "test restore: {:.2} s, {tried}",
        started.elapsed().as_secs_f64()
    );
    call(
        &app,
        "DELETE",
        &format!("/admin/users/{}", user_id(&people[5])),
        Some(&admin),
        None,
    )
    .await;
    let started = Instant::now();
    let (_, back) = call(
        &app,
        "POST",
        "/admin/backups/restore-person",
        Some(&admin),
        Some(json!({ "snapshot": id, "username": "person5" })),
    )
    .await;
    println!(
        "one person back: {:.2} s, {back}",
        started.elapsed().as_secs_f64()
    );
}
