// SPDX-License-Identifier: AGPL-3.0-or-later

//! Update notices from a signed release list served by a test host, and the
//! dashboard's security checklist.

use app_protocol::releases::{Release, ReleaseList, signed_message};
use axum::http::StatusCode;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};

use super::tests::{admin_on, call, server_with, with_admin};
use crate::config::Config;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn release(version: &str, security: bool, key: &SigningKey) -> Release {
    let mut release = Release {
        version: version.into(),
        date: "2026-11-01".into(),
        security,
        notes: format!("what {version} changes"),
        url: "https://example.org/notes".into(),
        signature: String::new(),
    };
    release.signature = hex(&key.sign(&signed_message(&release)).to_bytes());
    release
}

/// Serves `list` at `/releases.json` on a free port, returns its address.
async fn host(list: ReleaseList) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new().route(
        "/releases.json",
        axum::routing::get(move || {
            let list = list.clone();
            async move { axum::Json(list) }
        }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("http://{addr}/releases.json")
}

#[tokio::test]
async fn a_newer_signed_release_is_shown_and_forged_ones_are_not() {
    let key = SigningKey::from_bytes(&[5; 32]);
    let stranger = SigningKey::from_bytes(&[6; 32]);
    let mut forged = release("9.0.0", true, &key);
    forged.notes = "changed after signing".into();
    let list = ReleaseList {
        format: 1,
        releases: vec![
            release("0.1.0", false, &key),
            release("0.2.0", false, &key),
            release("8.0.0", true, &stranger),
            forged,
        ],
    };
    let mut config = Config::default();
    config.updates.url = host(list).await;
    config.updates.public_key = hex(key.verifying_key().as_bytes());
    let (_, app, token) = admin_on(server_with(config).await).await;

    let (status, view) = call(&app, "GET", "/admin/updates", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["enabled"], true);
    assert_eq!(view["available"], true);
    assert!(view.get("checkedAt").is_none(), "nothing asked yet");

    let (status, view) = call(&app, "POST", "/admin/updates/check", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["latest"]["version"], "0.2.0", "only signed entries");
    // a dev build compares with nothing
    assert_eq!(view["newer"], false);
    assert_eq!(view["security"], false);
    assert!(view.get("error").is_none(), "{view}");

    let (status, view) = call(
        &app,
        "PUT",
        "/admin/updates",
        Some(&token),
        Some(json!({ "enabled": false })),
    )
    .await;
    assert_eq!((status, &view["enabled"]), (StatusCode::OK, &json!(false)));
    let (status, _) = call(&app, "POST", "/admin/updates/check", Some(&token), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "off means no asking");
}

#[tokio::test]
async fn an_unreachable_list_keeps_the_last_good_one_and_says_why() {
    let key = SigningKey::from_bytes(&[5; 32]);
    let mut config = Config::default();
    config.updates.url = "http://127.0.0.1:9/releases.json".into();
    config.updates.public_key = hex(key.verifying_key().as_bytes());
    let (_, app, token) = admin_on(server_with(config).await).await;
    let (status, view) = call(&app, "POST", "/admin/updates/check", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        view["error"].as_str().unwrap().contains("cannot reach"),
        "{view}"
    );
    assert!(view.get("latest").is_none());
}

#[tokio::test]
async fn without_a_key_or_with_check_off_the_server_never_asks() {
    let mut config = Config::default();
    config.updates.public_key = String::new();
    let (_, app, token) = admin_on(server_with(config).await).await;
    let (_, view) = call(&app, "GET", "/admin/updates", Some(&token), None).await;
    assert_eq!(view["available"], false);
    let (status, _) = call(&app, "POST", "/admin/updates/check", Some(&token), None).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let mut config = Config::default();
    config.updates.check = false;
    config.updates.public_key = "ab".repeat(32);
    let (_, app, token) = admin_on(server_with(config).await).await;
    let (_, view) = call(&app, "GET", "/admin/updates", Some(&token), None).await;
    assert_eq!(view["available"], false);
}

fn line<'a>(list: &'a Value, key: &str) -> Option<&'a Value> {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["key"] == key)
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one story, each line of the list in turn"
)]
async fn the_checklist_says_what_to_fix() {
    let (state, app, token) = with_admin().await;
    let (status, list) = call(&app, "GET", "/admin/checklist", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let state_of = |key: &str| line(&list, key).map(|i| i["state"].as_str().unwrap().to_owned());
    assert_eq!(state_of("secondFactors").as_deref(), Some("fail"));
    assert_eq!(
        line(&list, "secondFactors").unwrap()["values"]["names"],
        "admin"
    );
    assert_eq!(state_of("registration").as_deref(), Some("ok"));
    assert_eq!(state_of("backups").as_deref(), Some("warn"));
    assert_eq!(line(&list, "backups").unwrap()["values"]["state"], "none");
    assert_eq!(
        state_of("tls").as_deref(),
        Some("fail"),
        "the test request is plain http"
    );
    assert_eq!(state_of("address").as_deref(), Some("fail"));
    assert_eq!(state_of("secrets").as_deref(), Some("warn"));

    // an admin with a factor, open registration, the reminder hidden
    let (_, setup) = call(&app, "POST", "/me/totp/setup", Some(&token), None).await;
    let code =
        crate::accounts::totp::code_now(setup["secret"].as_str().unwrap(), state.clock.now());
    let (status, _) = call(
        &app,
        "POST",
        "/me/totp/confirm",
        Some(&token),
        Some(json!({ "code": code, "password": "admin password" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    call(
        &app,
        "PUT",
        "/admin/registration",
        Some(&token),
        Some(json!({ "registration": "open" })),
    )
    .await;
    let (status, _) = call(
        &app,
        "POST",
        "/admin/checklist/dismiss",
        Some(&token),
        Some(json!({ "key": "secondFactors" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a line that is fine has nothing to hide"
    );
    // a line an admin finds fine as it is hides, as it stands now
    let (status, _) = call(
        &app,
        "POST",
        "/admin/checklist/dismiss",
        Some(&token),
        Some(json!({ "key": "registration" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, "GET", "/admin/checklist", Some(&token), None).await;
    assert_eq!(line(&list, "registration").unwrap()["dismissed"], true);
    assert!(line(&list, "tls").unwrap().get("dismissed").is_none());
    let (status, _) = call(&app, "POST", "/admin/checklist/restore", Some(&token), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, "GET", "/admin/checklist", Some(&token), None).await;
    assert!(
        line(&list, "registration")
            .unwrap()
            .get("dismissed")
            .is_none()
    );
    let (status, _) = call(
        &app,
        "POST",
        "/admin/checklist/dismiss",
        Some(&token),
        Some(json!({ "key": "secrets" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, "GET", "/admin/checklist", Some(&token), None).await;
    assert_eq!(line(&list, "secondFactors").unwrap()["state"], "ok");
    assert_eq!(line(&list, "registration").unwrap()["state"], "warn");
    assert!(line(&list, "secrets").is_none());

    // a failing backup, then a good one gone stale
    let master = state.master.clone();
    state
        .db
        .call(move |c| {
            let config = crate::backups::BackupConfig {
                target: serde_json::from_value(json!({ "kind": "folder", "path": "/nowhere" }))
                    .unwrap(),
                password: "p".into(),
                schedule: serde_json::from_value(json!("daily")).unwrap(),
                policy: serde_json::from_value(
                    json!({ "last": 1, "daily": 1, "weekly": 0, "monthly": 0, "yearly": 0 }),
                )
                .unwrap(),
                offset_seconds: 0,
            };
            crate::backups::set_config(c, &master, Some(&config))?;
            crate::backups::set_status(
                c,
                &crate::backups::BackupStatus {
                    last_run_at: Some(10),
                    last_success_at: Some(5),
                    last_error: Some("disk full".into()),
                    ..Default::default()
                },
            )
        })
        .await
        .unwrap();
    let (_, list) = call(&app, "GET", "/admin/checklist", Some(&token), None).await;
    let backups = line(&list, "backups").unwrap();
    assert_eq!(
        (&backups["state"], &backups["values"]["error"]),
        (&json!("fail"), &json!("disk full"))
    );
    state
        .db
        .call(|c| {
            crate::backups::set_status(
                c,
                &crate::backups::BackupStatus {
                    last_run_at: Some(10),
                    last_success_at: Some(10),
                    ..Default::default()
                },
            )
        })
        .await
        .unwrap();
    let (_, list) = call(&app, "GET", "/admin/checklist", Some(&token), None).await;
    assert_eq!(line(&list, "backups").unwrap()["values"]["state"], "late");
}

#[tokio::test]
async fn behind_https_with_an_address_the_way_in_is_fine() {
    let mut config = Config::default();
    config.server.public_url = "https://notes.example.org".into();
    let (_, app, token) = admin_on(server_with(config).await).await;
    let mut req = axum::http::Request::get("/api/v1/admin/checklist")
        .header(app_protocol::PROTOCOL_HEADER, "1")
        .header("authorization", format!("Bearer {token}"))
        .header("x-forwarded-proto", "https")
        .body(axum::body::Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::new(
            "127.0.0.1".parse().unwrap(),
            1,
        )));
    let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let list: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(line(&list, "tls").unwrap()["state"], "ok");
    assert_eq!(line(&list, "address").unwrap()["state"], "ok");
}

#[tokio::test]
async fn the_checklist_warns_when_storage_has_no_limit() {
    let (_, app, token) = with_admin().await;
    let (_, list) = call(&app, "GET", "/admin/checklist", Some(&token), None).await;
    let storage = line(&list, "storage").unwrap();
    assert_eq!(
        (&storage["state"], &storage["values"]["mb"]),
        (&json!("ok"), &json!("1024")),
        "1 GB per person by default"
    );

    let mut config = Config::default();
    config.files.quota_mb = 0;
    let (_, app, token) = admin_on(server_with(config).await).await;
    let (_, list) = call(&app, "GET", "/admin/checklist", Some(&token), None).await;
    let storage = line(&list, "storage").unwrap();
    assert_eq!(
        (&storage["state"], &storage["values"]["state"]),
        (&json!("warn"), &json!("unlimited"))
    );
}
