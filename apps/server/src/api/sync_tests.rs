// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sync through the full router, with real vaults of the app core as the
//! devices: what one device writes reaches the other through the server.

use app_core::model::{NoteScope, NoteSort};
use app_core::store::{NewNote, NoteChange, Store};
use app_core::vault::{Encryption, Vault};
use axum::Router;
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use super::tests::{call, device, login, with_admin};
use crate::routes::AppState;

const PASSWORD: &str = "admin password";

/// A device: a vault of its own, signed in to the account.
struct Device {
    store: Store,
    token: String,
    cursor: i64,
    _dir: tempfile::TempDir,
}

async fn device_of(app: &Router, username: &str, password: &str) -> Device {
    let dir = tempfile::tempdir().unwrap();
    let (_, store) = Vault::create(dir.path(), Encryption::Off).unwrap();
    let (status, session) = login(app, username, password).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    Device {
        store,
        token: session["accessToken"].as_str().unwrap().into(),
        cursor: 0,
        _dir: dir,
    }
}

impl Device {
    /// Sends the outbox, acked once the server has it.
    async fn push(&mut self, app: &Router) -> Value {
        let out = self.store.sync_outbox(usize::MAX).unwrap();
        let changes: Vec<Value> = out
            .iter()
            .map(|o| json!({ "id": o.id, "doc": o.doc_id, "data": STANDARD.encode(&o.bytes) }))
            .collect();
        let (status, answer) = call(
            app,
            "POST",
            "/sync/push",
            Some(&self.token),
            Some(json!({ "changes": changes })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        if let Some(last) = out.last() {
            self.store.sync_ack(last.seq).unwrap();
        }
        answer
    }

    /// Takes in everything after the cursor, page by page.
    async fn pull(&mut self, app: &Router, limit: usize) -> usize {
        let mut taken = 0;
        loop {
            let (status, page) = call(
                app,
                "GET",
                &format!("/sync/changes?after={}&limit={limit}", self.cursor),
                Some(&self.token),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{page}");
            for change in page["changes"].as_array().unwrap() {
                let bytes = STANDARD.decode(change["data"].as_str().unwrap()).unwrap();
                self.store
                    .apply_remote(change["doc"].as_str().unwrap(), &bytes)
                    .unwrap();
                self.cursor = change["seq"].as_i64().unwrap();
                taken += 1;
            }
            if !page["more"].as_bool().unwrap() {
                return taken;
            }
        }
    }

    fn view(&self) -> Vec<(String, String, Option<String>, bool)> {
        let mut notes: Vec<_> = self
            .store
            .list_notes(NoteScope::All, NoteSort::TitleAsc)
            .unwrap()
            .into_iter()
            .map(|n| {
                let full = self.store.note(n.id).unwrap();
                (
                    full.title,
                    full.body,
                    full.folder_id.map(|f| f.to_string()),
                    full.pinned,
                )
            })
            .collect();
        notes.sort();
        notes
    }
}

#[tokio::test]
async fn two_devices_of_one_account_end_the_same_through_the_server() {
    let (_, app, _) = with_admin().await;
    let mut laptop = device_of(&app, "admin", PASSWORD).await;
    let mut phone = device_of(&app, "admin", PASSWORD).await;

    let folder = laptop.store.create_folder(None, "Garden").unwrap();
    for i in 0..12 {
        let note = laptop
            .store
            .create_note(&NewNote {
                title: format!("note {i}"),
                body: format!("body {i}"),
                ..NewNote::default()
            })
            .unwrap();
        if i % 2 == 0 {
            laptop.store.move_note(note.id, Some(folder.id)).unwrap();
        }
    }
    let answer = laptop.push(&app).await;
    assert!(answer["accepted"].as_u64().unwrap() > 12);
    // small pages, so paging is exercised
    let taken = phone.pull(&app, 5).await;
    assert!(taken > 12);
    assert_eq!(phone.view(), laptop.view());

    // both edit offline, then meet again through the server
    let id = phone
        .store
        .list_notes(NoteScope::All, NoteSort::TitleAsc)
        .unwrap()[0]
        .id;
    phone
        .store
        .update_note(
            id,
            NoteChange {
                body: Some("body 0, from the phone".into()),
                ..NoteChange::default()
            },
        )
        .unwrap();
    laptop.store.set_pinned(id, true).unwrap();
    phone.push(&app).await;
    laptop.push(&app).await;
    laptop.pull(&app, 50).await;
    phone.pull(&app, 50).await;
    assert_eq!(phone.view(), laptop.view());
    let note = laptop.store.note(id).unwrap();
    assert!(note.pinned);
    assert_eq!(note.body, "body 0, from the phone");
}

#[tokio::test]
async fn a_change_sent_twice_is_kept_once() {
    let (_, app, _) = with_admin().await;
    let mut laptop = device_of(&app, "admin", PASSWORD).await;
    laptop.store.create_note(&NewNote::default()).unwrap();
    let out = laptop.store.sync_outbox(usize::MAX).unwrap();
    let body = json!({ "changes": out.iter().map(|o| json!({ "id": o.id, "doc": o.doc_id, "data": STANDARD.encode(&o.bytes) })).collect::<Vec<_>>() });
    let (_, first) = call(
        &app,
        "POST",
        "/sync/push",
        Some(&laptop.token),
        Some(body.clone()),
    )
    .await;
    let (_, again) = call(&app, "POST", "/sync/push", Some(&laptop.token), Some(body)).await;
    assert_eq!(first["accepted"], json!(out.len()));
    assert_eq!(again["accepted"], 0);
    assert_eq!(again["duplicates"], json!(out.len()));
    assert_eq!(again["head"], first["head"]);
}

#[tokio::test]
async fn pulls_name_the_change_at_the_cursor_and_big_changes_end_a_page() {
    let (_, app, _) = with_admin().await;
    let laptop = device_of(&app, "admin", PASSWORD).await;
    // three changes of 3 MB: a page stops near 8 MB of changes
    let big = STANDARD.encode(vec![7u8; 3 * 1024 * 1024]);
    let changes: Vec<Value> = (1..=3)
        .map(|i| json!({ "id": format!("p-{i}"), "doc": "space", "data": big }))
        .collect();
    let (status, _) = call(
        &app,
        "POST",
        "/sync/push",
        Some(&laptop.token),
        Some(json!({ "changes": changes })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, page) = call(&app, "GET", "/sync/changes", Some(&laptop.token), None).await;
    assert_eq!(page["changes"].as_array().unwrap().len(), 2, "8 MB a page");
    assert_eq!(page["more"], true);
    assert!(page.get("anchor").is_none(), "nothing before the start");
    assert_eq!(page["changes"][0]["id"], "p-1");
    let second = page["changes"][1]["seq"].as_i64().unwrap();
    let (_, rest) = call(
        &app,
        "GET",
        &format!("/sync/changes?after={second}"),
        Some(&laptop.token),
        None,
    )
    .await;
    assert_eq!(
        rest["anchor"], "p-2",
        "the change under the cursor, by name"
    );
    assert_eq!(rest["changes"][0]["id"], "p-3");
    assert_eq!(rest["more"], false);
    let (_, beyond) = call(
        &app,
        "GET",
        "/sync/changes?after=999999",
        Some(&laptop.token),
        None,
    )
    .await;
    assert!(
        beyond.get("anchor").is_none(),
        "no change under that number"
    );
}

#[tokio::test]
async fn standard_changes_are_encrypted_at_rest_with_the_master_key() {
    let (state, app, _) = with_admin().await;
    let mut laptop = device_of(&app, "admin", PASSWORD).await;
    laptop
        .store
        .create_note(&NewNote {
            title: "Secret recipe".into(),
            body: "two spoons of honey".into(),
            ..NewNote::default()
        })
        .unwrap();
    laptop.push(&app).await;
    let stored: Vec<Vec<u8>> = state
        .db
        .call(|c| {
            let mut stmt = c.prepare("SELECT bytes FROM sync_changes")?;
            Ok(stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
        .unwrap();
    assert!(!stored.is_empty());
    for bytes in &stored {
        assert!(
            !bytes.windows(5).any(|w| w == b"honey"),
            "plain text found in the database"
        );
    }
    // the same database with another master key cannot read the changes
    let mut other = AppState::new(crate::config::Config::default(), state.db.clone());
    other.master = std::sync::Arc::new(crate::keys::MasterKey::generate());
    let other_app = crate::routes::router(other);
    let (status, _) = call(
        &other_app,
        "GET",
        "/sync/changes",
        Some(&laptop.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn e2ee_changes_are_kept_as_sent_with_their_keys() {
    let (_, app, admin) = with_admin().await;
    let (_, _) = call(
        &app,
        "PUT",
        "/admin/registration",
        Some(&admin),
        Some(json!({ "registration": "open" })),
    )
    .await;
    // an e2ee account, made through the real OPAQUE exchange
    let start = app_e2ee::registration_start("e2ee password").unwrap();
    let (_, response) = call(
        &app,
        "POST",
        "/auth/opaque/registration",
        None,
        Some(json!({ "username": "zoe", "request": start.request })),
    )
    .await;
    let finish = app_e2ee::registration_finish(
        &start.state,
        "e2ee password",
        response["response"].as_str().unwrap(),
        "zoe",
        app_e2ee::Ksf::CURRENT,
    )
    .unwrap();
    let account = app_e2ee::new_account(&finish.export_key);
    let (status, session) = call(
        &app,
        "POST",
        "/auth/e2ee/register",
        None,
        Some(json!({ "username": "zoe", "ksf": 2, "upload": finish.upload, "keys": account.bundle, "recoveryVerifier": account.recovery_verifier, "device": device("phone") })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{session}");
    let token = session["accessToken"].as_str().unwrap();

    let (_, info) = call(&app, "GET", "/sync", Some(token), None).await;
    assert_eq!(info["kind"], "e2ee");
    let ciphertext = STANDARD.encode(b"device made ciphertext");
    let doc = "01a17735-9774-750e-9d3f-c1185429282d";
    let (status, answer) = call(
        &app,
        "POST",
        "/sync/push",
        Some(token),
        Some(json!({ "changes": [{ "id": "p-1", "doc": doc, "data": ciphertext, "key": "wrapped-note-key" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let (_, page) = call(&app, "GET", "/sync/changes", Some(token), None).await;
    assert_eq!(page["changes"][0]["data"], ciphertext);
    assert_eq!(page["keys"][0]["doc"], doc);
    assert_eq!(page["keys"][0]["wrapped"], "wrapped-note-key");

    // a standard account cannot send document keys
    let laptop = device_of(&app, "admin", PASSWORD).await;
    let (status, _) = call(
        &app,
        "POST",
        "/sync/push",
        Some(&laptop.token),
        Some(json!({ "changes": [{ "id": "a-1", "doc": doc, "data": ciphertext, "key": "k" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_account_sees_only_its_own_space_and_bad_changes_are_refused() {
    let (_, app, admin) = with_admin().await;
    let mut laptop = device_of(&app, "admin", PASSWORD).await;
    laptop.store.create_note(&NewNote::default()).unwrap();
    laptop.push(&app).await;
    call(
        &app,
        "POST",
        "/admin/users",
        Some(&admin),
        Some(json!({ "username": "bob", "password": "bob password" })),
    )
    .await;
    let bob = device_of(&app, "bob", "bob password").await;
    let (_, info) = call(&app, "GET", "/sync", Some(&bob.token), None).await;
    assert_eq!(info["head"], 0, "nothing of admin's space");
    let (_, page) = call(&app, "GET", "/sync/changes", Some(&bob.token), None).await;
    assert!(page["changes"].as_array().unwrap().is_empty());

    for bad in [
        json!({ "id": "", "doc": "space", "data": "AA==" }),
        json!({ "id": "x", "doc": "../etc", "data": "AA==" }),
        json!({ "id": "x", "doc": "space", "data": "not base64!" }),
        json!({ "id": "x", "doc": "space", "data": "" }),
    ] {
        let (status, body) = call(
            &app,
            "POST",
            "/sync/push",
            Some(&bob.token),
            Some(json!({ "changes": [bad] })),
        )
        .await;
        assert_eq!(
            (status, body["error"].as_str()),
            (StatusCode::BAD_REQUEST, Some("changeInvalid")),
            "{body}"
        );
    }
    let (status, _) = call(&app, "GET", "/sync", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

impl Device {
    /// Sends the outbox but never hears back, as when a connection drops
    /// after the server took the changes: the outbox stays, and the next
    /// push sends the same changes again.
    async fn push_answer_lost(&mut self, app: &Router) {
        let out = self.store.sync_outbox(usize::MAX).unwrap();
        let changes: Vec<Value> = out
            .iter()
            .map(|o| json!({ "id": o.id, "doc": o.doc_id, "data": STANDARD.encode(&o.bytes) }))
            .collect();
        call(
            app,
            "POST",
            "/sync/push",
            Some(&self.token),
            Some(json!({ "changes": changes })),
        )
        .await;
    }
}

/// Three devices of one account change things at random, push and pull at
/// random moments in small pages, and lose some push answers (so they send
/// the same changes again). In the end, all equal and nothing left to send.
/// `SERVER_SYNC_FUZZ_RUNS` raises the count.
#[tokio::test]
async fn random_devices_through_the_server_end_the_same() {
    use app_core::testing::{Rng, random_change, view};
    let runs: u64 = std::env::var("SERVER_SYNC_FUZZ_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    for seed in 1..=runs {
        let (_, app, _) = with_admin().await;
        let mut devices = Vec::new();
        for _ in 0..3 {
            devices.push(device_of(&app, "admin", PASSWORD).await);
        }
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for _ in 0..60 {
            let d = rng.below(3);
            random_change(&mut devices[d].store, &mut rng);
            match rng.below(6) {
                0 => {
                    devices[d].push(&app).await;
                }
                1 => devices[d].push_answer_lost(&app).await,
                2 => {
                    let limit = 1 + rng.below(4);
                    devices[d].pull(&app, limit).await;
                }
                _ => {}
            }
        }
        for device in &mut devices {
            device.push(&app).await;
        }
        for device in &mut devices {
            device.pull(&app, 7).await;
        }
        let views: Vec<String> = devices.iter().map(|d| view(&d.store)).collect();
        for (i, device) in devices.iter_mut().enumerate() {
            assert!(
                device.store.sync_outbox(1).unwrap().is_empty(),
                "seed {seed}: device {i} has changes left"
            );
            assert_eq!(views[0], views[i], "seed {seed}: device {i} differs");
        }
    }
}

/// The server as the client sync engine sees it, through the full router.
pub(super) struct Wire {
    app: Router,
    token: String,
    rt: tokio::runtime::Handle,
}

impl Wire {
    fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, app_core::sync::engine::RemoteError> {
        use app_core::sync::engine::RemoteError;
        let (status, answer) =
            self.rt
                .block_on(call(&self.app, method, path, Some(&self.token), body));
        match status {
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => Ok(answer),
            StatusCode::UNAUTHORIZED => Err(RemoteError::SignedOut),
            _ => Err(RemoteError::Refused(answer.to_string())),
        }
    }

    /// A raw request, for the file calls: status and body.
    fn raw(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> (StatusCode, Vec<u8>) {
        use tower::ServiceExt;
        let mut req = axum::http::Request::builder()
            .method(method)
            .uri(format!("/api/v1{path}"))
            .header(
                app_protocol::PROTOCOL_HEADER,
                app_protocol::PROTOCOL_VERSION.to_string(),
            )
            .header("authorization", format!("Bearer {}", self.token));
        for (name, value) in headers {
            req = req.header(*name, value);
        }
        let app = self.app.clone();
        self.rt.block_on(async move {
            let response = app
                .oneshot(req.body(axum::body::Body::from(body)).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), 256 << 20)
                .await
                .unwrap();
            (status, bytes.to_vec())
        })
    }

    fn file_answer<T: serde::de::DeserializeOwned>(
        status: StatusCode,
        body: &[u8],
    ) -> Result<T, app_core::sync::files::FileError> {
        use app_core::sync::engine::RemoteError;
        use app_core::sync::files::FileError;
        match status {
            StatusCode::OK => Ok(serde_json::from_slice(body).unwrap()),
            StatusCode::NOT_FOUND => Err(FileError::Missing),
            StatusCode::UNAUTHORIZED => Err(FileError::Remote(RemoteError::SignedOut)),
            _ => {
                let error: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
                Err(FileError::Refused(
                    error["error"].as_str().unwrap_or("refused").to_owned(),
                ))
            }
        }
    }
}

impl app_core::sync::engine::Remote for Wire {
    fn push(
        &mut self,
        space: Option<&str>,
        request: &app_protocol::sync::PushRequest,
    ) -> Result<app_protocol::sync::PushAnswer, app_core::sync::engine::RemoteError> {
        let answer = self.call(
            "POST",
            &format!("/sync/push{}", in_space(space, '?')),
            Some(serde_json::to_value(request).unwrap()),
        )?;
        Ok(serde_json::from_value(answer).unwrap())
    }

    fn pull(
        &mut self,
        space: Option<&str>,
        after: i64,
        limit: usize,
    ) -> Result<app_protocol::sync::PullAnswer, app_core::sync::engine::RemoteError> {
        let answer = self.call(
            "GET",
            &format!(
                "/sync/changes?after={after}&limit={limit}{}",
                in_space(space, '&')
            ),
            None,
        )?;
        Ok(serde_json::from_value(answer).unwrap())
    }

    fn heavy(
        &mut self,
        space: Option<&str>,
        min: i64,
    ) -> Result<app_protocol::sync::HeavyDocs, app_core::sync::engine::RemoteError> {
        let answer = self.call(
            "GET",
            &format!("/sync/heavy?min={min}{}", in_space(space, '&')),
            None,
        )?;
        Ok(serde_json::from_value(answer).unwrap())
    }

    fn spaces(
        &mut self,
    ) -> Result<Option<app_protocol::sync::SharedSpaces>, app_core::sync::engine::RemoteError> {
        let answer = self.call("GET", "/sync/spaces", None)?;
        Ok(Some(serde_json::from_value(answer).unwrap()))
    }

    fn give_grants(
        &mut self,
        space: &str,
        grants: &app_protocol::sync::GiveGrants,
    ) -> Result<(), app_core::sync::engine::RemoteError> {
        self.call(
            "POST",
            &format!("/sync/spaces/{space}/grants"),
            Some(serde_json::to_value(grants).unwrap()),
        )
        .map(drop)
    }

    fn files_info(
        &mut self,
    ) -> Result<Option<app_protocol::files::FilesInfo>, app_core::sync::engine::RemoteError> {
        let answer = self.call("GET", "/sync/files/policy", None)?;
        Ok(Some(serde_json::from_value(answer).unwrap()))
    }

    fn file_state(
        &mut self,
        space: Option<&str>,
        address: &str,
    ) -> Result<app_protocol::files::FileState, app_core::sync::files::FileError> {
        let path = format!("/sync/files/{address}/state{}", in_space(space, '?'));
        let (status, body) = self.raw("GET", &path, &[], Vec::new());
        Self::file_answer(status, &body)
    }

    fn put_file(
        &mut self,
        space: Option<&str>,
        address: &str,
        part: app_core::sync::files::FilePart<'_>,
    ) -> Result<app_protocol::files::FileState, app_core::sync::files::FileError> {
        let range = if part.total == 0 {
            "bytes */0".to_owned()
        } else {
            format!(
                "bytes {}-{}/{}",
                part.start,
                part.start + part.bytes.len() as u64 - 1,
                part.total
            )
        };
        let mut headers = vec![("content-range", range)];
        if let Some((name, mime)) = part.declared {
            headers.push(("x-file-name", app_protocol::files::encode_name(name)));
            headers.push(("x-file-type", mime.to_owned()));
        }
        let path = format!("/sync/files/{address}{}", in_space(space, '?'));
        let (status, body) = self.raw("PUT", &path, &headers, part.bytes.to_vec());
        Self::file_answer(status, &body)
    }

    fn get_file(
        &mut self,
        space: Option<&str>,
        address: &str,
    ) -> Result<Box<dyn std::io::Read + Send>, app_core::sync::files::FileError> {
        let path = format!("/sync/files/{address}{}", in_space(space, '?'));
        let (status, body) = self.raw("GET", &path, &[], Vec::new());
        if status == StatusCode::OK {
            return Ok(Box::new(std::io::Cursor::new(body)));
        }
        Self::file_answer::<Value>(status, &body).map(|_| unreachable!())
    }

    fn keep_file(
        &mut self,
        space: Option<&str>,
        address: &str,
    ) -> Result<(), app_core::sync::files::FileError> {
        let path = format!("/sync/files/{address}/keep{}", in_space(space, '?'));
        let (status, body) = self.raw("POST", &path, &[], Vec::new());
        if status == StatusCode::NO_CONTENT {
            return Ok(());
        }
        Self::file_answer::<Value>(status, &body).map(drop)
    }

    fn release_file(
        &mut self,
        space: Option<&str>,
        address: &str,
        head: i64,
    ) -> Result<bool, app_core::sync::files::FileError> {
        let path = format!("/sync/files/{address}?head={head}{}", in_space(space, '&'));
        let (status, body) = self.raw("DELETE", &path, &[], Vec::new());
        match status {
            StatusCode::NO_CONTENT => Ok(true),
            StatusCode::CONFLICT => Ok(false),
            _ => Self::file_answer::<Value>(status, &body).map(|_| false),
        }
    }
}

/// `space=` for a shared space's calls, nothing for the personal space.
fn in_space(space: Option<&str>, sep: char) -> String {
    space.map(|s| format!("{sep}space={s}")).unwrap_or_default()
}

/// A vault linked from a sign-in answer, and its wire.
pub(super) fn linked(
    app: &Router,
    session: &Value,
    master: Option<&app_e2ee::MasterKey>,
) -> (std::sync::Mutex<Store>, Wire, tempfile::TempDir) {
    use app_core::store::{NewLink, Tokens};
    let dir = tempfile::tempdir().unwrap();
    let (_, mut store) = Vault::create(dir.path(), Encryption::Off).unwrap();
    store
        .sync_set_link(&NewLink {
            server: "http://test".into(),
            account_id: session["user"]["id"].as_str().unwrap().into(),
            username: session["user"]["username"].as_str().unwrap().into(),
            kind: serde_json::from_value(session["user"]["kind"].clone()).unwrap(),
            master_key: master.map(|k| app_e2ee::MasterKey::from_slice(k.as_bytes()).unwrap()),
            device_id: session["deviceId"].as_str().unwrap().into(),
            tokens: Tokens {
                access: session["accessToken"].as_str().unwrap().into(),
                access_expires_at: session["accessExpiresAt"].as_i64().unwrap(),
                refresh: session["refreshToken"].as_str().unwrap().into(),
                refresh_expires_at: session["refreshExpiresAt"].as_i64().unwrap(),
            },
        })
        .unwrap();
    let wire = Wire {
        app: app.clone(),
        token: session["accessToken"].as_str().unwrap().into(),
        rt: tokio::runtime::Handle::current(),
    };
    (std::sync::Mutex::new(store), wire, dir)
}

/// Runs a sync round off the async threads, as the apps do.
pub(super) async fn round(
    device: (std::sync::Mutex<Store>, Wire, tempfile::TempDir),
) -> (
    (std::sync::Mutex<Store>, Wire, tempfile::TempDir),
    app_core::sync::engine::Round,
) {
    tokio::task::spawn_blocking(move || {
        let (store, mut wire, dir) = device;
        // the owner chose its spaces' members in the app's share dialog,
        // which lets them in; these tests add them through the api
        if let Ok(Some(listed)) = app_core::sync::engine::Remote::spaces(&mut wire) {
            let mut s = store.lock().unwrap();
            for space in &listed.spaces {
                for wanted in &space.grants_wanted {
                    if !s.space_trusted(&space.id, &wanted.user_id).unwrap() {
                        s.space_trust(&space.id, &wanted.user_id, None).unwrap();
                    }
                }
            }
        }
        let round = app_core::sync::engine::run(&store, &mut wire).unwrap();
        ((store, wire, dir), round)
    })
    .await
    .unwrap()
}

pub(super) fn view_of(device: &(std::sync::Mutex<Store>, Wire, tempfile::TempDir)) -> String {
    app_core::testing::view(&device.0.lock().unwrap())
}

#[tokio::test(flavor = "multi_thread")]
#[expect(clippy::too_many_lines, reason = "one story, read top to bottom")]
async fn the_client_engine_syncs_standard_and_e2ee_accounts_through_the_server() {
    let (_, app, admin) = with_admin().await;
    call(
        &app,
        "PUT",
        "/admin/registration",
        Some(&admin),
        Some(json!({ "registration": "open" })),
    )
    .await;

    // standard: two sign-ins of the admin
    let (_, s1) = login(&app, "admin", PASSWORD).await;
    let (_, s2) = login(&app, "admin", PASSWORD).await;
    let a = linked(&app, &s1, None);
    let b = linked(&app, &s2, None);
    let mut rng = app_core::testing::Rng(7);
    for _ in 0..80 {
        app_core::testing::random_change(&mut a.0.lock().unwrap(), &mut rng);
        app_core::testing::random_change(&mut b.0.lock().unwrap(), &mut rng);
    }
    let (a, pushed) = round(a).await;
    assert!(pushed.pushed > 0);
    let (b, _) = round(b).await;
    let (a, _) = round(a).await;
    assert_eq!(view_of(&a), view_of(&b));

    // e2ee: sign up, then a second device signs in and opens the keys
    let start = app_e2ee::registration_start("e2ee password").unwrap();
    let (_, response) = call(
        &app,
        "POST",
        "/auth/opaque/registration",
        None,
        Some(json!({ "username": "zoe", "request": start.request })),
    )
    .await;
    let finish = app_e2ee::registration_finish(
        &start.state,
        "e2ee password",
        response["response"].as_str().unwrap(),
        "zoe",
        app_e2ee::Ksf::CURRENT,
    )
    .unwrap();
    let made = app_e2ee::new_account(&finish.export_key);
    let (status, z1) = call(
        &app,
        "POST",
        "/auth/e2ee/register",
        None,
        Some(json!({ "username": "zoe", "ksf": 2, "upload": finish.upload, "keys": made.bundle, "recoveryVerifier": made.recovery_verifier, "device": device("phone") })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{z1}");
    let master =
        app_e2ee::AccountKeys::open_with_password(&made.bundle, &finish.export_key).unwrap();
    let phone = linked(&app, &z1, Some(&master));
    let start = app_e2ee::login_start("e2ee password").unwrap();
    let (_, started) = call(
        &app,
        "POST",
        "/auth/opaque/login/start",
        None,
        Some(json!({ "username": "zoe", "request": start.request })),
    )
    .await;
    let done = app_e2ee::login_finish(
        &start.state,
        "e2ee password",
        started["response"].as_str().unwrap(),
        "zoe",
        super::tests::ksf(&started),
    )
    .unwrap();
    let (status, z2) = call(
        &app,
        "POST",
        "/auth/opaque/login/finish",
        None,
        Some(json!({ "challenge": started["challenge"], "finalization": done.finalization, "device": device("laptop") })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{z2}");
    let token = z2["accessToken"].as_str().unwrap();
    let (_, keys) = call(&app, "GET", "/me/keys", Some(token), None).await;
    let bundle: app_e2ee::KeyBundle = serde_json::from_value(keys["keys"].clone()).unwrap();
    let opened = app_e2ee::AccountKeys::open_with_password(&bundle, &done.export_key).unwrap();
    let laptop = linked(&app, &z2, Some(&opened));

    phone
        .0
        .lock()
        .unwrap()
        .create_note(&NewNote {
            title: "Diary".into(),
            body: "only my devices read this".into(),
            ..NewNote::default()
        })
        .unwrap();
    let (phone, _) = round(phone).await;
    let (laptop, came) = round(laptop).await;
    assert!(
        came.taken.applied > 0 && came.taken.rejected == 0,
        "{came:?}"
    );
    assert_eq!(view_of(&phone), view_of(&laptop));
    assert!(view_of(&laptop).contains("only my devices read this"));
}

/// A live socket on a server listening for real.
pub(super) async fn live_socket(
    addr: std::net::SocketAddr,
    token: Option<&str>,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::Error,
> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = format!("ws://{addr}/api/v1/sync/live")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("x-protocol-version", "1".parse().unwrap());
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
    }
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(s, _)| s)
}

/// The next text message, skipping pings, or `None` after `wait`.
pub(super) async fn next_message(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    wait: std::time::Duration,
) -> Option<Result<Value, Option<u16>>> {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    let end = tokio::time::Instant::now() + wait;
    loop {
        let message = tokio::time::timeout_at(end, socket.next()).await.ok()?;
        match message {
            Some(Ok(Message::Text(text))) => return Some(Ok(serde_json::from_str(&text).unwrap())),
            Some(Ok(Message::Close(frame))) => return Some(Err(frame.map(|f| u16::from(f.code)))),
            Some(Ok(_)) => {}
            Some(Err(_)) | None => return Some(Err(None)),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn live_sockets_hear_of_other_devices_changes_and_the_admin_setting() {
    let (_, app, admin) = with_admin().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = app.clone();
    tokio::spawn(async move {
        axum::serve(
            listener,
            served.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let second = std::time::Duration::from_secs(2);
    let quiet = std::time::Duration::from_millis(400);

    assert!(live_socket(addr, None).await.is_err(), "a token is needed");

    let mut phone = device_of(&app, "admin", PASSWORD).await;
    let mut laptop = device_of(&app, "admin", PASSWORD).await;
    let mut socket = live_socket(addr, Some(&laptop.token)).await.unwrap();
    let hello = next_message(&mut socket, second).await.unwrap().unwrap();
    assert_eq!(
        hello,
        json!({ "type": "hello", "head": 0, "coediting": true })
    );

    phone.store.create_note(&NewNote::default()).unwrap();
    let answer = phone.push(&app).await;
    let changed = next_message(&mut socket, second).await.unwrap().unwrap();
    assert_eq!(changed["type"], "changed");
    assert_eq!(changed["head"], answer["head"]);

    // its own pushes are not announced back, nor a push of nothing new
    laptop.store.create_note(&NewNote::default()).unwrap();
    laptop.push(&app).await;
    phone.push(&app).await;
    assert!(
        next_message(&mut socket, quiet).await.is_none(),
        "nothing to hear"
    );

    // another account's changes are not heard either
    call(
        &app,
        "POST",
        "/admin/users",
        Some(&admin),
        Some(json!({ "username": "bob", "password": "bob password" })),
    )
    .await;
    let mut bob = device_of(&app, "bob", "bob password").await;
    bob.store.create_note(&NewNote::default()).unwrap();
    bob.push(&app).await;
    assert!(
        next_message(&mut socket, quiet).await.is_none(),
        "bob's space"
    );

    // the admin turns co-editing off: connected devices hear it, the space says it
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/coediting",
        Some(&admin),
        Some(json!({ "coediting": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let settings = next_message(&mut socket, second).await.unwrap().unwrap();
    assert_eq!(settings, json!({ "type": "settings", "coediting": false }));
    let (_, info) = call(&app, "GET", "/sync", Some(&laptop.token), None).await;
    assert_eq!(info["coediting"], false);
    let (_, overview) = call(&app, "GET", "/admin/overview", Some(&admin), None).await;
    assert_eq!(overview["coediting"], false);
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/coediting",
        Some(&bob.token),
        Some(json!({ "coediting": true })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "admins only");

    // the device is signed out elsewhere: the socket closes for it
    let (_, devices) = call(&app, "GET", "/me/devices", Some(&phone.token), None).await;
    let laptop_id = devices
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["current"] == false)
        .map(|d| d["id"].as_str().unwrap().to_owned())
        .max()
        .unwrap();
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/me/devices/{laptop_id}"),
        Some(&phone.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let closed = next_message(&mut socket, second).await.unwrap();
    assert_eq!(closed, Err(Some(4401)), "closed with sign in again");
}

#[tokio::test(flavor = "multi_thread")]
async fn compaction_through_the_server_empties_old_changes_and_new_devices_still_get_all() {
    let (state, app, _) = with_admin().await;
    let (_, s1) = login(&app, "admin", PASSWORD).await;
    let (_, s2) = login(&app, "admin", PASSWORD).await;
    let a = linked(&app, &s1, None);
    let id =
        a.0.lock()
            .unwrap()
            .create_note(&NewNote {
                title: "Diary".into(),
                body: "day 0".into(),
                ..NewNote::default()
            })
            .unwrap()
            .id;
    let (mut a, _) = round(a).await;
    for day in 1..=40 {
        a.0.lock()
            .unwrap()
            .update_note(
                id,
                NoteChange {
                    body: Some(format!("day {day}")),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        (a, _) = round(a).await;
    }
    let live = |doc: String| {
        let db = state.db.clone();
        async move {
            db.call(move |c| {
                Ok(c.query_row(
                    "SELECT count(*), coalesce(sum(size), 0) FROM sync_changes WHERE doc_id = ?1 AND compacted = 0",
                    [doc],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                )?)
            })
            .await
            .unwrap()
        }
    };
    let (before, bytes_before) = live(id.to_string()).await;
    assert!(before > 40);
    let a = tokio::task::spawn_blocking(move || {
        let (store, mut wire, dir) = a;
        let queued = app_core::sync::engine::compact(&store, &mut wire, 20).unwrap();
        assert!(queued >= 1);
        app_core::sync::engine::run(&store, &mut wire).unwrap();
        (store, wire, dir)
    })
    .await
    .unwrap();
    let (after, bytes_after) = live(id.to_string()).await;
    assert_eq!(after, 1, "one whole note in place of {before} changes");
    assert!(
        bytes_after < bytes_before,
        "{bytes_before} bytes became {bytes_after}"
    );
    let b = linked(&app, &s2, None);
    let (b, _) = round(b).await;
    assert_eq!(view_of(&a), view_of(&b));
    assert!(view_of(&b).contains("day 40"));

    // a compaction naming a change the space does not have is refused
    let token = s1["accessToken"].as_str().unwrap();
    let (status, _) = call(
        &app,
        "POST",
        "/sync/push",
        Some(token),
        Some(json!({ "changes": [{ "id": "x-1", "doc": "space", "data": "AAE=", "compacts": 999_999 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Timings for the gate, release build:
/// `cargo test -p server --release sync_costs -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement, run by hand"]
#[expect(clippy::too_many_lines, reason = "one measurement, read top to bottom")]
async fn sync_costs_through_the_server() {
    use std::time::Instant;
    let (state, app, _) = with_admin().await;
    let (_, s1) = login(&app, "admin", PASSWORD).await;
    let (_, s2) = login(&app, "admin", PASSWORD).await;
    let (_, s3) = login(&app, "admin", PASSWORD).await;
    let a = linked(&app, &s1, None);
    let body = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(18);
    let t = Instant::now();
    {
        let mut store = a.0.lock().unwrap();
        for i in 0..2000 {
            store
                .create_note(&NewNote {
                    title: format!("Note {i}"),
                    body: body.clone(),
                    ..NewNote::default()
                })
                .unwrap();
        }
    }
    println!("2000 notes of 1 KB written: {:?}", t.elapsed());
    let t = Instant::now();
    let (a, first) = round(a).await;
    println!(
        "first push of 2000 notes: {:?}, {} changes",
        t.elapsed(),
        first.pushed
    );
    let stored = |state: &AppState| {
        let db = state.db.clone();
        async move {
            db.call(|c| {
                Ok(c.query_row(
                    "SELECT count(*), coalesce(sum(size), 0) FROM sync_changes WHERE compacted = 0",
                    [],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                )?)
            })
            .await
            .unwrap()
        }
    };
    println!("on the server: {:?} (changes, bytes)", stored(&state).await);
    let b = linked(&app, &s2, None);
    let t = Instant::now();
    let (b, round_b) = round(b).await;
    println!(
        "new device takes 2000 notes: {:?}, {} applied",
        t.elapsed(),
        round_b.taken.applied
    );
    assert_eq!(view_of(&a), view_of(&b));

    // one edit there and back
    let id =
        a.0.lock()
            .unwrap()
            .list_notes(NoteScope::All, NoteSort::TitleAsc)
            .unwrap()[0]
            .id;
    a.0.lock()
        .unwrap()
        .update_note(
            id,
            NoteChange {
                body: Some(format!("{body} edited")),
                ..NoteChange::default()
            },
        )
        .unwrap();
    let t = Instant::now();
    let (mut a, _) = round(a).await;
    let push_one = t.elapsed();
    let t = Instant::now();
    let (mut b, _) = round(b).await;
    println!(
        "one edit: push {push_one:?}, pull on the other device {:?}",
        t.elapsed()
    );

    // a note edited 500 times, then compacted
    for i in 0..500 {
        a.0.lock()
            .unwrap()
            .update_note(
                id,
                NoteChange {
                    body: Some(format!("{body} {i}")),
                    ..NoteChange::default()
                },
            )
            .unwrap();
    }
    (a, _) = round(a).await;
    (b, _) = round(b).await;
    println!("after 500 edits of one note: {:?}", stored(&state).await);
    let t = Instant::now();
    let a = tokio::task::spawn_blocking(move || {
        let (store, mut wire, dir) = a;
        app_core::sync::engine::compact(&store, &mut wire, 200).unwrap();
        app_core::sync::engine::run(&store, &mut wire).unwrap();
        (store, wire, dir)
    })
    .await
    .unwrap();
    println!(
        "compaction: {:?}, then {:?}",
        t.elapsed(),
        stored(&state).await
    );
    let c = linked(&app, &s3, None);
    let t = Instant::now();
    let (c, _) = round(c).await;
    println!("new device after compaction: {:?}", t.elapsed());
    assert_eq!(view_of(&a), view_of(&c));
    let _ = b;
}

/// One push of `size` random bytes as a change of `doc`, `compacts` set for
/// a compaction.
async fn push_bytes(
    app: &Router,
    token: &str,
    id: &str,
    size: usize,
    compacts: Option<i64>,
) -> (StatusCode, Value) {
    let bytes: Vec<u8> = (0..size)
        .map(|i| (i * 7 + id.len()).to_le_bytes()[0])
        .collect();
    let mut change = json!({
        "id": id,
        "doc": "00000000-0000-0000-0000-000000000001",
        "data": STANDARD.encode(&bytes),
    });
    if let Some(n) = compacts {
        change["compacts"] = json!(n);
    }
    call(
        app,
        "POST",
        "/sync/push",
        Some(token),
        Some(json!({ "changes": [change] })),
    )
    .await
}

#[tokio::test]
async fn note_changes_count_against_the_storage_quota() {
    let config = crate::config::Config {
        files: crate::config::FilesConfig {
            quota_mb: 2,
            ..crate::config::FilesConfig::default()
        },
        ..crate::config::Config::default()
    };
    let (_, app, token) = super::tests::admin_on(super::tests::server_with(config).await).await;
    for i in 0..2 {
        let (status, answer) = push_bytes(&app, &token, &format!("c{i}"), 900_000, None).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
    }
    let (status, answer) = push_bytes(&app, &token, "c2", 900_000, None).await;
    assert_eq!(status, StatusCode::INSUFFICIENT_STORAGE, "{answer}");
    assert_eq!(answer["error"], "quotaFull");
    // a compaction frees room, it is never refused for want of it
    let (status, answer) = push_bytes(&app, &token, "c3", 900_000, Some(2)).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    // and then there is room again
    let (status, answer) = push_bytes(&app, &token, "c4", 500_000, None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
}

/// Calling a push a compaction does not get it
/// past the quota, only the bytes it empties do.
#[tokio::test]
async fn a_fake_compaction_does_not_get_past_the_quota() {
    let config = crate::config::Config {
        files: crate::config::FilesConfig {
            quota_mb: 2,
            ..crate::config::FilesConfig::default()
        },
        ..crate::config::Config::default()
    };
    let (_, app, token) = super::tests::admin_on(super::tests::server_with(config).await).await;
    for i in 0..2 {
        let (status, answer) = push_bytes(&app, &token, &format!("c{i}"), 900_000, None).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
    }
    // a "compaction" of a document that has nothing to empty
    let bytes = vec![7u8; 900_000];
    let (status, answer) = call(
        &app,
        "POST",
        "/sync/push",
        Some(&token),
        Some(json!({ "changes": [{
            "id": "fake",
            "doc": "00000000-0000-0000-0000-000000000002",
            "data": STANDARD.encode(&bytes),
            "compacts": 2,
        }] })),
    )
    .await;
    assert_eq!(status, StatusCode::INSUFFICIENT_STORAGE, "{answer}");
    // a real one frees what it replaces, a bigger one pays the difference
    let (status, answer) = push_bytes(&app, &token, "real", 2_200_000, Some(2)).await;
    assert_eq!(status, StatusCode::INSUFFICIENT_STORAGE, "{answer}");
    let (status, answer) = push_bytes(&app, &token, "real", 1_000_000, Some(2)).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
}
