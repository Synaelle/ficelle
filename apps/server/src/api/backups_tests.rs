// SPDX-License-Identifier: AGPL-3.0-or-later

//! The instance's backups through the full router: set up by an admin,
//! run, listed, checked, test restored; a whole instance restored into an
//! empty folder.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::files_tests::{content, sha, upload};
use super::tests::{admin_on, call, person, server_with, token};
use crate::config::{Config, StorageConfig};

// a server runs one backup at a time, process wide, so these tests take turns
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
async fn an_instance_is_backed_up_test_restored_and_restored_whole() {
    let _turn = ONE_AT_A_TIME.lock().await;
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
    let kim = person(&state, "kim").await;
    let push = json!({ "changes": [
        { "id": "c1", "doc": "space", "data": "AAE=" },
        { "id": "c2", "doc": "space", "data": "AAI=" },
    ] });
    let (status, _) = call(&app, "POST", "/sync/push", Some(&admin), Some(push)).await;
    assert_eq!(status, StatusCode::OK);
    let bytes = content(300_000, 7);
    let (status, body) = upload(&app, &admin, None, &sha(&bytes), &bytes, 64 * 1024).await;
    assert!(status.is_success(), "{body}");

    // only an admin sets backups up
    let (status, _) = set_up(&app, token(&kim), place.path(), true, "pw").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, view) = set_up(&app, &admin, place.path(), true, "backup pw").await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["config"]["keep"], 30);
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
    // the feedback database stays out: deleted reports must not live on
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
    assert_eq!(view["status"]["lastReport"]["users"], 2);
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
    assert_eq!(list[0]["counts"]["users"], 2);
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
    assert_eq!(tried["users"], 2);
    assert_eq!(tried["files"], 1);
    let left: Vec<_> = std::fs::read_dir(data.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("backup-"))
        .collect();
    assert!(left.is_empty(), "scratch folders are removed");

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
    assert_eq!(restored.counts["users"], 2);
    let db = crate::db::Db::open(&into.path().join("data")).unwrap();
    let users: i64 = db
        .call(|c| Ok(c.query_row("SELECT count(*) FROM users", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(users, 2);
    let feedback = crate::db::Db::open_feedback(&into.path().join("data")).unwrap();
    let words: i64 = feedback
        .call(|c| Ok(c.query_row("SELECT count(*) FROM blocks", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(words, 0, "the feedback database starts empty");
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
    let _turn = ONE_AT_A_TIME.lock().await;
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

#[tokio::test(flavor = "multi_thread")]
async fn older_settings_keep_their_schedule_and_policy_until_a_number_is_chosen() {
    let _turn = ONE_AT_A_TIME.lock().await;
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
    let (status, _) = set_up(&app, &admin, place.path(), true, "backup pw").await;
    assert_eq!(status, StatusCode::OK);
    for _ in 0..3 {
        state.clock.advance(crate::clock::MINUTE);
        call(&app, "POST", "/admin/backups/run", Some(&admin), None).await;
    }
    // as the earlier admin page left them: hourly, the newest one and one a day
    let master = state.master.clone();
    state
        .db
        .call(move |c| {
            let mut config = crate::backups::config(c, &master)?.unwrap();
            config.schedule = crate::backups::Schedule::Hourly;
            config.policy = app_repo::Policy {
                last: 1,
                daily: 7,
                weekly: 4,
                monthly: 12,
                yearly: 0,
            };
            crate::backups::set_config(c, &master, Some(&config))
        })
        .await
        .unwrap();
    let (_, view) = call(&app, "GET", "/admin/backups", Some(&admin), None).await;
    assert_eq!(view["config"]["keep"], 3, "what is there: {view}");
    let master = state.master.clone();
    let kept = state
        .db
        .call(move |c| Ok(crate::backups::config(c, &master)?.unwrap()))
        .await
        .unwrap();
    assert_eq!(kept.schedule, crate::backups::Schedule::Hourly);
    assert_eq!(kept.keep(), None);

    let (status, view) = call(
        &app,
        "PUT",
        "/admin/backups/settings",
        Some(&admin),
        Some(json!({ "keep": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let (_, view) = call(&app, "POST", "/admin/backups/run", Some(&admin), None).await;
    assert_eq!(view["status"]["snapshots"], 3, "{view}");
    let master = state.master.clone();
    let now = state
        .db
        .call(move |c| Ok(crate::backups::config(c, &master)?.unwrap()))
        .await
        .unwrap();
    assert_eq!(now.schedule, crate::backups::Schedule::Daily);
    assert_eq!(now.keep(), Some(3));
}
