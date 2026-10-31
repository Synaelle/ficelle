// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment files through the full router: uploads in parts, resumed,
//! checked, downloads and ranges, quotas, sizes and types, who may read or
//! write, release, keeping and the sweep.

use app_protocol::files::{FileStatus, encode_name};
use app_protocol::{PROTOCOL_HEADER, PROTOCOL_VERSION};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use super::files::status_of;
use super::sharing_tests::{e2ee, open_registration, share, standard, token, user_id};
use super::tests::{call, login, server_with, with_admin};
use crate::clock::DAY;
use crate::config::{Config, FilesConfig};
use crate::routes::AppState;

const CHUNK: usize = 64 * 1024;

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

pub(super) fn sha(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(super) fn content(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| u8::try_from(i % 251).unwrap() ^ seed)
        .collect()
}

/// A request with raw bytes and headers, answered with its status, headers
/// and body.
pub(super) async fn raw(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    headers: &[(&str, String)],
    body: Vec<u8>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut req = Request::builder()
        .method(method)
        .uri(format!("/api/v1{path}"))
        .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
        .header("authorization", format!("Bearer {token}"));
    for (name, value) in headers {
        req = req.header(*name, value);
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 256 << 20).await.unwrap();
    (status, headers, bytes.to_vec())
}

fn in_space(space: Option<&str>, sep: char) -> String {
    space.map(|s| format!("{sep}space={s}")).unwrap_or_default()
}

/// Sends one part, returns the status and the answer.
#[expect(
    clippy::too_many_arguments,
    reason = "a test helper, each one says what it sends"
)]
async fn part(
    app: &Router,
    token: &str,
    space: Option<&str>,
    address: &str,
    bytes: &[u8],
    start: usize,
    total: usize,
    name: &str,
) -> (StatusCode, Value) {
    let range = if total == 0 {
        "bytes */0".to_owned()
    } else {
        format!("bytes {start}-{}/{total}", start + bytes.len() - 1)
    };
    let (status, _, body) = raw(
        app,
        "PUT",
        &format!("/sync/files/{address}{}", in_space(space, '?')),
        token,
        &[
            ("content-range", range),
            ("x-file-name", encode_name(name)),
            ("x-file-type", "application/octet-stream".into()),
        ],
        bytes.to_vec(),
    )
    .await;
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// Uploads a whole file in parts of `per` bytes.
pub(super) async fn upload(
    app: &Router,
    token: &str,
    space: Option<&str>,
    address: &str,
    bytes: &[u8],
    per: usize,
) -> (StatusCode, Value) {
    if bytes.is_empty() {
        return part(app, token, space, address, bytes, 0, 0, "empty").await;
    }
    let mut last = (StatusCode::OK, Value::Null);
    for start in (0..bytes.len()).step_by(per) {
        let end = (start + per).min(bytes.len());
        last = part(
            app,
            token,
            space,
            address,
            &bytes[start..end],
            start,
            bytes.len(),
            "file.bin",
        )
        .await;
        if last.0 != StatusCode::OK {
            return last;
        }
    }
    last
}

async fn download(
    app: &Router,
    token: &str,
    space: Option<&str>,
    address: &str,
    range: Option<&str>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let headers: Vec<(&str, String)> = range.map(|r| ("range", r.to_owned())).into_iter().collect();
    raw(
        app,
        "GET",
        &format!("/sync/files/{address}{}", in_space(space, '?')),
        token,
        &headers,
        Vec::new(),
    )
    .await
}

async fn file_state(app: &Router, token: &str, space: Option<&str>, address: &str) -> Value {
    let (status, state) = call(
        app,
        "GET",
        &format!("/sync/files/{address}/state{}", in_space(space, '?')),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{state}");
    state
}

async fn objects(state: &AppState) -> i64 {
    state
        .db
        .call(|c| Ok(c.query_row("SELECT count(*) FROM files", [], |r| r.get(0))?))
        .await
        .unwrap()
}

async fn head(app: &Router, token: &str) -> i64 {
    let (_, info) = call(app, "GET", "/sync", Some(token), None).await;
    info["head"].as_i64().unwrap()
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one upload followed from start to end"
)]
async fn a_file_goes_up_in_parts_resumes_and_comes_back_whole_or_in_ranges() {
    let (state, app, admin) = with_admin().await;
    let bytes = content(3 * CHUNK + 1000, 1);
    let address = sha(&bytes);
    assert_eq!(
        status_of(&file_state(&app, &admin, None, &address).await),
        FileStatus::None
    );
    // the first part, then the connection drops
    let (status, answer) = part(
        &app,
        &admin,
        None,
        &address,
        &bytes[..2 * CHUNK],
        0,
        bytes.len(),
        "a.bin",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let now = file_state(&app, &admin, None, &address).await;
    assert_eq!(status_of(&now), FileStatus::Partial);
    assert_eq!(now["received"], 2 * CHUNK);
    // a part out of place says where the upload stands
    let (status, _) = part(
        &app,
        &admin,
        None,
        &address,
        &bytes[3 * CHUNK..],
        3 * CHUNK,
        bytes.len(),
        "a.bin",
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    // parts not cut at chunks are refused for standard files
    let (status, _) = part(
        &app,
        &admin,
        None,
        &address,
        &bytes[2 * CHUNK..2 * CHUNK + 10],
        2 * CHUNK,
        bytes.len(),
        "a.bin",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // resumed where it stood
    let (status, answer) = part(
        &app,
        &admin,
        None,
        &address,
        &bytes[2 * CHUNK..],
        2 * CHUNK,
        bytes.len(),
        "a.bin",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(status_of(&answer), FileStatus::Complete);

    let (status, headers, back) = download(&app, &admin, None, &address, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-length"], bytes.len().to_string().as_str());
    assert_eq!(back, bytes);
    let (status, headers, back) = download(
        &app,
        &admin,
        None,
        &address,
        Some(&format!("bytes={}-", CHUNK + 7)),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        headers["content-range"],
        format!("bytes {}-{}/{}", CHUNK + 7, bytes.len() - 1, bytes.len()).as_str()
    );
    assert_eq!(back, bytes[CHUNK + 7..]);
    let (status, _, back) = download(&app, &admin, None, &address, Some("bytes=10-19")).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(back, bytes[10..20]);
    let (status, _, _) = download(&app, &admin, None, &address, Some("bytes=999999999-")).await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);

    // sealed at rest: the stored object is not the content
    let object = state.files.read(&address, 0).unwrap();
    let mut stored = Vec::new();
    std::io::Read::read_to_end(&mut { object }, &mut stored).unwrap();
    assert_eq!(
        stored.len(),
        usize::try_from(app_e2ee::sealed_size(bytes.len() as u64)).unwrap()
    );
    assert!(
        !stored
            .windows(64)
            .any(|w| bytes.windows(64).next() == Some(w))
    );
    assert_eq!(objects(&state).await, 1);

    // an empty file
    let empty = sha(b"");
    let (status, answer) = upload(&app, &admin, None, &empty, b"", CHUNK).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let (status, _, back) = download(&app, &admin, None, &empty, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(back.is_empty());
}

#[tokio::test]
async fn content_that_is_not_its_address_is_not_kept() {
    let (state, app, admin) = with_admin().await;
    let bytes = content(CHUNK + 5, 2);
    let claimed = sha(b"something else");
    let (status, answer) = upload(&app, &admin, None, &claimed, &bytes, CHUNK).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(answer["error"], "fileInvalid");
    assert_eq!(
        status_of(&file_state(&app, &admin, None, &claimed).await),
        FileStatus::None
    );
    assert_eq!(objects(&state).await, 0);
    assert!(state.files.incoming().unwrap().is_empty());
}

#[tokio::test]
async fn the_same_file_from_two_accounts_is_stored_once_each_uploading_it() {
    let (state, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
    let bytes = content(2 * CHUNK, 3);
    let address = sha(&bytes);
    upload(&app, &admin, None, &address, &bytes, CHUNK).await;
    // alex cannot read it without having uploaded it
    let (status, _, _) = download(&app, token(&alex), None, &address, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        status_of(&file_state(&app, token(&alex), None, &address).await),
        FileStatus::None
    );
    let (status, answer) = upload(&app, token(&alex), None, &address, &bytes, CHUNK).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(objects(&state).await, 1);
    let (_, _, back) = download(&app, token(&alex), None, &address, None).await;
    assert_eq!(back, bytes);
    // both count it
    for t in [admin.as_str(), token(&alex)] {
        let (_, info) = call(&app, "GET", "/sync/files/policy", Some(t), None).await;
        assert_eq!(info["usage"]["fileBytes"], 2 * CHUNK);
        assert_eq!(info["usage"]["files"], 1);
        assert_eq!(info["partBytes"], 8 * 1024 * 1024);
    }
}

#[tokio::test]
async fn sizes_quotas_and_types_are_enforced() {
    let config = Config {
        files: FilesConfig {
            max_file_mb: 1,
            quota_mb: 2,
            blocked_types: vec![".exe".into(), "video/*".into()],
            ..FilesConfig::default()
        },
        ..Config::default()
    };
    let (state, app, admin) = super::tests::admin_on(server_with(config).await).await;
    let big = content(1024 * 1024 + 1, 4);
    let (status, answer) = upload(&app, &admin, None, &sha(&big), &big, 8 * CHUNK).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{answer}");
    assert_eq!(answer["error"], "fileTooLarge");

    // two files of 900 KB fit, a third does not
    let mut kept = 0;
    for seed in 0..3u8 {
        let bytes = content(900 * 1024, 10 + seed);
        let (status, answer) = upload(&app, &admin, None, &sha(&bytes), &bytes, 8 * CHUNK).await;
        if seed < 2 {
            assert_eq!(status, StatusCode::OK, "{answer}");
            kept += 1;
        } else {
            assert_eq!(status, StatusCode::INSUFFICIENT_STORAGE, "{answer}");
            assert_eq!(answer["error"], "quotaFull");
        }
    }
    assert_eq!(objects(&state).await, kept);

    // blocked by name, by type, and by what the bytes are
    let exe = b"MZ\x90\x00 a program".to_vec();
    let address = sha(&exe);
    let (status, answer) = part(
        &app,
        &admin,
        None,
        &address,
        &exe,
        0,
        exe.len(),
        "setup.exe",
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{answer}");
    assert_eq!(answer["error"], "fileTypeRefused");
    let (status, _) = part(&app, &admin, None, &address, &exe, 0, exe.len(), "cat.png").await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let (status, _, body) = raw(
        &app,
        "PUT",
        &format!("/sync/files/{}", sha(b"clip")),
        &admin,
        &[
            ("content-range", "bytes 0-3/4".into()),
            ("x-file-name", "clip.bin".into()),
            ("x-file-type", "video/mp4".into()),
        ],
        b"clip".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body:?}");
    let text = b"plain notes".to_vec();
    let (status, _) = part(
        &app,
        &admin,
        None,
        &sha(&text),
        &text,
        0,
        text.len(),
        "notes.txt",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn members_read_editors_write_and_strangers_see_nothing() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let reader = standard(&app, "reader").await;
    let editor = standard(&app, "editor").await;
    let stranger = standard(&app, "stranger").await;
    let (_, me) = call(&app, "GET", "/me", Some(&admin), None).await;
    let _ = me;
    let space = share(
        &app,
        &json!({ "accessToken": admin }),
        json!([
            { "userId": user_id(&reader), "role": "read" },
            { "userId": user_id(&editor), "role": "edit" }
        ]),
    )
    .await;
    let bytes = content(CHUNK, 5);
    let address = sha(&bytes);
    let (status, _) = upload(&app, token(&reader), Some(&space), &address, &bytes, CHUNK).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, answer) =
        upload(&app, token(&editor), Some(&space), &address, &bytes, CHUNK).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let (status, _, back) = download(&app, token(&reader), Some(&space), &address, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(back, bytes);
    let (status, _, _) = download(&app, token(&stranger), Some(&space), &address, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // the editor's upload counts against the owner
    let (_, info) = call(&app, "GET", "/sync/files/policy", Some(&admin), None).await;
    assert_eq!(info["usage"]["files"], 1);
    let (_, info) = call(
        &app,
        "GET",
        "/sync/files/policy",
        Some(token(&editor)),
        None,
    )
    .await;
    assert_eq!(info["usage"]["files"], 0);
}

#[tokio::test]
async fn e2ee_files_are_kept_as_sent_and_never_typed() {
    let config = Config {
        files: FilesConfig {
            allowed_types: vec!["image/*".into()],
            ..FilesConfig::default()
        },
        ..Config::default()
    };
    let (state, app, admin) = super::tests::admin_on(server_with(config).await).await;
    open_registration(&app, &admin).await;
    let (zoe, _device) = e2ee(&app, "zoe").await;
    let files = app_e2ee::files_key_from(&[9u8; 32]);
    let plain = content(2 * CHUNK + 3, 6);
    let digest = sha(&plain);
    let sealed = app_e2ee::seal_file(&files, &digest, &plain).unwrap();
    let address = app_e2ee::file_address(&files, &digest);
    // any part size, no name sent, the type rules do not apply
    let (status, answer) = upload(&app, token(&zoe), None, &address, &sealed, 50_000).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let (_, _, back) = download(&app, token(&zoe), None, &address, None).await;
    assert_eq!(back, sealed);
    assert_eq!(*app_e2ee::open_file(&files, &digest, &back).unwrap(), plain);
    // a size no sealed file has
    let (status, _) = upload(&app, token(&zoe), None, &sha(b"x"), &sealed[..30], 30).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let object = format!("e-{}-{address}", user_id(&zoe));
    let mut stored = Vec::new();
    std::io::Read::read_to_end(&mut state.files.read(&object, 0).unwrap(), &mut stored).unwrap();
    assert_eq!(stored, sealed);
}

#[tokio::test]
async fn a_release_waits_for_a_space_that_moved_on_then_the_file_goes_after_retention() {
    let (state, app, admin) = with_admin().await;
    let bytes = content(CHUNK, 7);
    let address = sha(&bytes);
    upload(&app, &admin, None, &address, &bytes, CHUNK).await;
    let seen = head(&app, &admin).await;
    // another device pushes a change meanwhile
    let (status, _) = call(
        &app,
        "POST",
        "/sync/push",
        Some(&admin),
        Some(json!({ "changes": [{ "id": "dev-1", "doc": "space", "data": "AQID" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let path = format!("/sync/files/{address}?head={seen}");
    let (status, answer) = call(&app, "DELETE", &path, Some(&admin), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(answer["error"], "spaceMovedOn");
    let now = head(&app, &admin).await;
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/sync/files/{address}?head={now}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        status_of(&file_state(&app, &admin, None, &address).await),
        FileStatus::Kept
    );
    // still readable while kept, and taken back without sending it
    let (status, _, _) = download(&app, &admin, None, &address, None).await;
    assert_eq!(status, StatusCode::OK);
    let keep = format!("/sync/files/{address}/keep");
    let (status, _) = call(&app, "POST", &keep, Some(&admin), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        status_of(&file_state(&app, &admin, None, &address).await),
        FileStatus::Complete
    );
    let (status, _) = call(&app, "POST", &keep, Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // released again: the sweep leaves it for the retention time
    call(
        &app,
        "DELETE",
        &format!("/sync/files/{address}?head={now}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(super::files::sweep(&state).await.unwrap(), 0);
    state.clock.advance(31 * DAY);
    assert_eq!(super::files::sweep(&state).await.unwrap(), 1);
    assert_eq!(objects(&state).await, 0);
    assert!(state.files.read(&address, 0).is_err());
    let (_, session) = login(&app, "admin", "admin password").await;
    assert_eq!(
        status_of(&file_state(&app, token(&session), None, &address).await),
        FileStatus::None
    );
}

#[tokio::test]
async fn abandoned_uploads_and_leftovers_are_swept() {
    let (state, app, admin) = with_admin().await;
    let bytes = content(3 * CHUNK, 8);
    let address = sha(&bytes);
    part(
        &app,
        &admin,
        None,
        &address,
        &bytes[..CHUNK],
        0,
        bytes.len(),
        "a",
    )
    .await;
    state
        .files
        .write_incoming("u-gone-leftover", 0, b"x")
        .unwrap();
    assert_eq!(state.files.incoming().unwrap().len(), 2);
    super::files::sweep(&state).await.unwrap();
    // the upload in progress stays, the leftover goes
    assert_eq!(state.files.incoming().unwrap().len(), 1);
    state.clock.advance(2 * DAY);
    super::files::sweep(&state).await.unwrap();
    assert!(state.files.incoming().unwrap().is_empty());
    let (_, session) = login(&app, "admin", "admin password").await;
    assert_eq!(
        status_of(&file_state(&app, token(&session), None, &address).await),
        FileStatus::None
    );
}

/// Released files kept for the retention time
/// count once they pass one more quota, and taking one back needs room.
#[tokio::test]
async fn released_files_cannot_be_used_to_go_past_the_quota() {
    let config = Config {
        files: FilesConfig {
            quota_mb: 2,
            ..FilesConfig::default()
        },
        ..Config::default()
    };
    let (_state, app, admin) = super::tests::admin_on(server_with(config).await).await;
    let release = |address: String| {
        let app = app.clone();
        let admin = admin.clone();
        async move {
            let now = head(&app, &admin).await;
            let path = format!("/sync/files/{address}?head={now}");
            let (status, _) = call(&app, "DELETE", &path, Some(&admin), None).await;
            assert_eq!(status, StatusCode::NO_CONTENT);
        }
    };
    // one quota of released files is free, the next counts
    let mut released = Vec::new();
    let mut refused = false;
    for seed in 0..6u8 {
        let bytes = content(900 * 1024, 30 + seed);
        let address = sha(&bytes);
        let (status, answer) = upload(&app, &admin, None, &address, &bytes, 8 * CHUNK).await;
        if status != StatusCode::OK {
            assert_eq!(answer["error"], "quotaFull", "{answer}");
            refused = true;
            break;
        }
        release(address.clone()).await;
        released.push(address);
    }
    assert!(refused, "released files stopped counting for free");
    assert!(released.len() >= 2, "{} released", released.len());

    // a file kept for its retention time comes back only with room for it
    let fill = content(900 * 1024, 50);
    let (status, answer) = upload(&app, &admin, None, &sha(&fill), &fill, 8 * CHUNK).await;
    assert_eq!(status, StatusCode::INSUFFICIENT_STORAGE, "{answer}");
    let keep = format!("/sync/files/{}/keep", released[0]);
    let (status, answer) = call(&app, "POST", &keep, Some(&admin), None).await;
    assert_eq!(status, StatusCode::INSUFFICIENT_STORAGE, "{answer}");
}
