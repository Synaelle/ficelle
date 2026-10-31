// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment files between real vaults of the app core and the full
//! router: uploads after the notes' round, fetches, releases, sealing for
//! e2ee accounts and shared spaces, refusals and quotas.

use app_core::blobs::BlobStore;
use app_core::id::NoteId;
use app_core::store::Item;
use app_core::sync::files::FilesRound;
use axum::http::StatusCode;
use serde_json::json;

use super::sharing_tests::{Device, e2ee, open_registration, share, standard, token, user_id};
use super::sync_tests::{linked, round};
use super::tests::{call, login, with_admin};
use crate::routes::AppState;

fn blobs(device: &Device) -> BlobStore {
    BlobStore::new(device.2.path().join("files"), None)
}

fn content(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| u8::try_from(i % 251).unwrap() ^ seed)
        .collect()
}

/// A note with a file attached, its id and the file's hash.
fn attach(device: &Device, title: &str, name: &str, bytes: &[u8]) -> (NoteId, String) {
    let sha = blobs(device).put(bytes).unwrap();
    let mut store = device.0.lock().unwrap();
    let note = store
        .create_note(&app_core::store::NewNote {
            title: title.into(),
            ..Default::default()
        })
        .unwrap();
    store
        .add_attachment(
            note.id,
            name,
            "application/octet-stream",
            i64::try_from(bytes.len()).unwrap(),
            &sha,
        )
        .unwrap();
    (note.id, sha)
}

/// A notes round then a files round, as the apps run them.
async fn sync(device: Device, fetch_up_to: u64) -> (Device, FilesRound) {
    let (device, _) = round(device).await;
    tokio::task::spawn_blocking(move || {
        let (store, mut wire, dir) = device;
        let files = BlobStore::new(dir.path().join("files"), None);
        let done = app_core::sync::files::transfer(&store, &files, &mut wire, fetch_up_to).unwrap();
        ((store, wire, dir), done)
    })
    .await
    .unwrap()
}

async fn objects(state: &AppState) -> Vec<String> {
    state
        .db
        .call(|c| {
            let mut stmt = c.prepare("SELECT object FROM files ORDER BY object")?;
            let rows = stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            Ok(rows)
        })
        .await
        .unwrap()
}

fn stored(state: &AppState, object: &str) -> Vec<u8> {
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut state.files.read(object, 0).unwrap(), &mut out).unwrap();
    out
}

const MB: usize = 1024 * 1024;

/// Another device of an e2ee account: a sign-in of its own, by OPAQUE.
async fn e2ee_login(app: &axum::Router, name: &str) -> serde_json::Value {
    let password = format!("{name} password");
    let start = app_e2ee::login_start(&password).unwrap();
    let (_, started) = call(
        app,
        "POST",
        "/auth/opaque/login/start",
        None,
        Some(json!({ "username": name, "request": start.request })),
    )
    .await;
    let done = app_e2ee::login_finish(
        &start.state,
        &password,
        started["response"].as_str().unwrap(),
        name,
        super::tests::ksf(&started),
    )
    .unwrap();
    let (status, session) = call(
        app,
        "POST",
        "/auth/opaque/login/finish",
        None,
        Some(json!({ "challenge": started["challenge"], "finalization": done.finalization, "device": super::tests::device("laptop") })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    session
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_attached_on_one_device_reaches_the_other_and_goes_when_removed() {
    let (state, app, _) = with_admin().await;
    let laptop = linked(&app, &login(&app, "admin", "admin password").await.1, None);
    let phone = linked(&app, &login(&app, "admin", "admin password").await.1, None);
    let bytes = content(3 * MB + 5, 1);
    let (note, sha) = attach(&laptop, "Scan", "scan.pdf", &bytes);
    let (laptop, done) = sync(laptop, 10 * MB as u64).await;
    assert_eq!(done.uploaded, 1);
    assert_eq!(objects(&state).await, vec![sha.clone()]);
    // standard: sealed at rest by the server
    assert_ne!(stored(&state, &sha)[..100], bytes[..100]);

    let (phone, done) = sync(phone, 10 * MB as u64).await;
    assert_eq!(done.fetched, 1);
    assert_eq!(blobs(&phone).get(&sha).unwrap().unwrap(), bytes);
    // nothing more to do
    let (phone, done) = sync(phone, 10 * MB as u64).await;
    assert_eq!(done, FilesRound::default());

    // removed on the phone: released once the removal is on the server
    let id = phone.0.lock().unwrap().list_attachments(note).unwrap()[0].id;
    phone.0.lock().unwrap().remove_attachment(id).unwrap();
    let (_phone, done) = sync(phone, 10 * MB as u64).await;
    assert_eq!(done.released, 1);
    let (_, laptop_done) = sync(laptop, 10 * MB as u64).await;
    assert_eq!(laptop_done.released, 1, "the laptop forgets it too");
    let released: i64 = state
        .db
        .call(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM file_refs WHERE released_at IS NOT NULL",
                [],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(released, 1, "kept for the retention time");
}

#[tokio::test(flavor = "multi_thread")]
async fn large_files_wait_until_asked_for_and_thumbnails_come_at_once() {
    let (_, app, _) = with_admin().await;
    let laptop = linked(&app, &login(&app, "admin", "admin password").await.1, None);
    let phone = linked(&app, &login(&app, "admin", "admin password").await.1, None);
    let big = content(2 * MB, 2);
    let thumb = content(5000, 3);
    let (note, sha) = attach(&laptop, "Photo", "photo.jpg", &big);
    let thumb_sha = blobs(&laptop).put(&thumb).unwrap();
    {
        let mut store = laptop.0.lock().unwrap();
        let id = store.list_attachments(note).unwrap()[0].id;
        store.set_attachment_thumb(id, &thumb_sha).unwrap();
    }
    let (_laptop, done) = sync(laptop, MB as u64).await;
    assert_eq!(done.uploaded, 2);
    let (phone, done) = sync(phone, MB as u64).await;
    assert_eq!(done.fetched, 1, "the thumbnail only");
    assert!(blobs(&phone).has(&thumb_sha).unwrap());
    assert!(!blobs(&phone).has(&sha).unwrap());
    let phone = tokio::task::spawn_blocking(move || {
        let (store, mut wire, dir) = phone;
        let files = BlobStore::new(dir.path().join("files"), None);
        app_core::sync::files::fetch(&store, &files, &mut wire, &sha).unwrap();
        assert_eq!(files.get(&sha).unwrap().unwrap(), big);
        (store, wire, dir)
    })
    .await
    .unwrap();
    drop(phone);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2ee_files_are_sealed_on_the_device_and_opened_on_the_other() {
    let (state, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let (zoe, laptop) = e2ee(&app, "zoe").await;
    let master = laptop.0.lock().unwrap().sync_master_key().unwrap().unwrap();
    let phone = linked(&app, &e2ee_login(&app, "zoe").await, Some(&master));
    drop(zoe);
    let bytes = content(MB + 77, 4);
    let (_, sha) = attach(&laptop, "Secret", "secret.png", &bytes);
    let (_laptop, done) = sync(laptop, 10 * MB as u64).await;
    assert_eq!(done.uploaded, 1);
    let held = objects(&state).await;
    assert_eq!(held.len(), 1);
    assert!(!held[0].contains(&sha), "the server never learns the hash");
    let raw = stored(&state, &held[0]);
    assert!(!raw.windows(64).any(|w| w == &bytes[..64]));
    let (phone, done) = sync(phone, 10 * MB as u64).await;
    assert_eq!(done.fetched, 1);
    assert_eq!(blobs(&phone).get(&sha).unwrap().unwrap(), bytes);
}

#[tokio::test(flavor = "multi_thread")]
async fn files_moved_into_an_e2ee_share_are_sealed_with_its_key_for_the_members() {
    let (state, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let (zoe, owner) = e2ee(&app, "zoe").await;
    let (yan, member) = e2ee(&app, "yan").await;
    let bytes = content(200_000, 5);
    let (note, sha) = attach(&owner, "Plans", "plans.pdf", &bytes);
    let (owner, _) = sync(owner, 10 * MB as u64).await;
    let space = share(
        &app,
        &zoe,
        json!([{ "userId": user_id(&yan), "role": "edit" }]),
    )
    .await;
    // the key first, then the move
    let (owner, _) = round(owner).await;
    owner
        .0
        .lock()
        .unwrap()
        .move_to_space(Item::Note(note), &space)
        .unwrap();
    let (owner, done) = sync(owner, 10 * MB as u64).await;
    assert_eq!(done.uploaded, 1);
    // the epoch went into the document: a second round sends it
    let (_owner, _) = sync(owner, 10 * MB as u64).await;
    let (member, done) = sync(member, 10 * MB as u64).await;
    assert_eq!(done.fetched, 1, "{done:?}");
    assert_eq!(blobs(&member).get(&sha).unwrap().unwrap(), bytes);
    let in_space: Vec<String> = objects(&state)
        .await
        .into_iter()
        .filter(|o| o.contains(&space))
        .collect();
    assert_eq!(in_space.len(), 1);
    drop((member, token(&zoe)));
}

#[tokio::test(flavor = "multi_thread")]
async fn refused_files_are_noted_once_and_a_full_quota_stops_uploads() {
    let (_, app, admin) = with_admin().await;
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/policy",
        Some(&admin),
        Some(json!({ "blockedTypes": [".exe"], "quotaMb": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
    let laptop = linked(&app, &alex, None);
    attach(&laptop, "Tool", "setup.exe", b"MZ a program");
    let (laptop, done) = sync(laptop, 10 * MB as u64).await;
    assert_eq!(done.refused, 1);
    let refused = laptop.0.lock().unwrap().files_refused().unwrap();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].1, "fileTypeRefused");
    // not tried again on the next round, and a full quota stops the space's uploads
    attach(&laptop, "Big", "big.bin", &content(2 * MB, 6));
    attach(&laptop, "Small", "small.txt", b"a few words");
    let (_laptop, done) = sync(laptop, 10 * MB as u64).await;
    assert_eq!(done.refused, 0);
    assert!(done.quota_full);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement, run by hand"]
async fn file_costs_through_the_server() {
    use std::time::Instant;
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let big = content(100 * MB, 9);
    for kind in ["standard", "e2ee"] {
        let (laptop, phone) = if kind == "standard" {
            let sam = standard(&app, "sam").await;
            (
                linked(&app, &sam, None),
                linked(&app, &login(&app, "sam", "sam password").await.1, None),
            )
        } else {
            let (zoe, laptop) = e2ee(&app, "zoe").await;
            let master = laptop.0.lock().unwrap().sync_master_key().unwrap().unwrap();
            drop(zoe);
            let phone = linked(&app, &e2ee_login(&app, "zoe").await, Some(&master));
            (laptop, phone)
        };
        let t = Instant::now();
        let (_, sha) = attach(&laptop, "Video", "film.bin", &big);
        println!("{kind}: store 100 MB in the vault: {:?}", t.elapsed());
        let t = Instant::now();
        let (_laptop, done) = sync(laptop, u64::MAX).await;
        assert_eq!(done.uploaded, 1);
        println!("{kind}: upload 100 MB: {:?}", t.elapsed());
        let t = Instant::now();
        let (phone, done) = sync(phone, u64::MAX).await;
        assert_eq!(done.fetched, 1);
        println!("{kind}: download 100 MB: {:?}", t.elapsed());
        assert!(blobs(&phone).has(&sha).unwrap());
    }
}
