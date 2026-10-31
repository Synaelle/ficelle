// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sharing through the full router, with real vaults of the app core as
//! the devices of different accounts.

use app_core::store::{Item, NewNote, NoteChange};
use axum::Router;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::sync_tests::{linked, round, view_of};
use super::tests::{call, device, login, with_admin};
use crate::routes::AppState;

pub(super) type Device = (
    std::sync::Mutex<app_core::store::Store>,
    super::sync_tests::Wire,
    tempfile::TempDir,
);

pub(super) async fn open_registration(app: &Router, admin: &str) {
    let (status, _) = call(
        app,
        "PUT",
        "/admin/registration",
        Some(admin),
        Some(json!({ "registration": "open" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// A standard account and its sign-in answer.
pub(super) async fn standard(app: &Router, name: &str) -> Value {
    let (status, session) = call(
        app,
        "POST",
        "/auth/register",
        None,
        Some(json!({ "username": name, "password": format!("{name} password"), "device": device("phone") })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{session}");
    session
}

/// An e2ee account with its key pair for sharing sent, and a linked device
/// holding both.
pub(super) async fn e2ee(app: &Router, name: &str) -> (Value, Device) {
    let password = format!("{name} password");
    let start = app_e2ee::registration_start(&password).unwrap();
    let (_, response) = call(
        app,
        "POST",
        "/auth/opaque/registration",
        None,
        Some(json!({ "username": name, "request": start.request })),
    )
    .await;
    let finish = app_e2ee::registration_finish(
        &start.state,
        &password,
        response["response"].as_str().unwrap(),
        name,
        app_e2ee::Ksf::CURRENT,
    )
    .unwrap();
    let made = app_e2ee::new_account(&finish.export_key);
    let (status, session) = call(
        app,
        "POST",
        "/auth/e2ee/register",
        None,
        Some(json!({ "username": name, "ksf": 2, "upload": finish.upload, "keys": made.bundle, "recoveryVerifier": made.recovery_verifier, "device": device("phone") })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{session}");
    let master =
        app_e2ee::AccountKeys::open_with_password(&made.bundle, &finish.export_key).unwrap();
    let pair = app_e2ee::new_key_pair();
    let token = session["accessToken"].as_str().unwrap();
    let (status, kept) = call(
        app,
        "PUT",
        "/sync/key-pair",
        Some(token),
        Some(json!({ "publicKey": pair.public_text(), "privateSealed": app_e2ee::seal_key_pair(&master, &pair) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{kept}");
    let device = linked(app, &session, Some(&master));
    let opened = app_e2ee::open_key_pair(
        &master,
        kept["publicKey"].as_str().unwrap(),
        kept["privateSealed"].as_str().unwrap(),
    )
    .unwrap();
    device.0.lock().unwrap().sync_set_key_pair(&opened).unwrap();
    (session, device)
}

pub(super) fn token(session: &Value) -> &str {
    session["accessToken"].as_str().unwrap()
}

pub(super) fn user_id(session: &Value) -> &str {
    session["user"]["id"].as_str().unwrap()
}

pub(super) async fn share(app: &Router, owner: &Value, members: Value) -> String {
    let (status, space) = call(
        app,
        "POST",
        "/sync/spaces",
        Some(token(owner)),
        Some(json!({ "members": members })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{space}");
    assert_eq!(space["role"], "owner");
    space["id"].as_str().unwrap().to_owned()
}

fn note(device: &Device, title: &str, body: &str) -> app_core::id::NoteId {
    device
        .0
        .lock()
        .unwrap()
        .create_note(&NewNote {
            title: title.into(),
            body: body.into(),
            ..NewNote::default()
        })
        .unwrap()
        .id
}

fn space_view(device: &Device, space: &str) -> String {
    app_core::testing::space_view(&device.0.lock().unwrap(), space)
}

#[tokio::test(flavor = "multi_thread")]
#[expect(clippy::too_many_lines, reason = "one story, read top to bottom")]
async fn a_shared_folder_goes_both_ways_between_two_standard_accounts() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
    let (_, sam) = login(&app, "admin", "admin password").await;
    let laptop = linked(&app, &sam, None);
    let phone = linked(&app, &alex, None);
    let folder = laptop
        .0
        .lock()
        .unwrap()
        .create_folder(None, "Trip")
        .unwrap();
    let id = note(&laptop, "Tickets", "to book");
    laptop
        .0
        .lock()
        .unwrap()
        .move_note(id, Some(folder.id))
        .unwrap();
    note(&laptop, "Private", "only mine");
    let (laptop, _) = round(laptop).await;

    let space = share(
        &app,
        &sam,
        json!([{ "userId": user_id(&alex), "role": "edit" }]),
    )
    .await;
    let (laptop, _) = round(laptop).await;
    let moved: app_core::id::FolderId = laptop
        .0
        .lock()
        .unwrap()
        .move_to_space(Item::Folder(folder.id), &space)
        .unwrap()
        .parse()
        .unwrap();
    let (laptop, _) = round(laptop).await;

    let (phone, came) = round(phone).await;
    assert_eq!(came.taken.rejected, 0, "{came:?}");
    assert_eq!(space_view(&laptop, &space), space_view(&phone, &space));
    assert!(view_of(&phone).contains("to book"));
    assert!(
        !view_of(&phone).contains("only mine"),
        "only what was shared"
    );
    let (_, listed) = call(&app, "GET", "/sync/spaces", Some(token(&alex)), None).await;
    assert_eq!(listed["spaces"][0]["role"], "edit");
    assert_eq!(listed["spaces"][0]["owner"], "admin");

    // alex edits, sam sees it
    let shared = phone
        .0
        .lock()
        .unwrap()
        .list_notes(
            app_core::model::NoteScope::All,
            app_core::model::NoteSort::TitleAsc,
        )
        .unwrap()[0]
        .id;
    phone
        .0
        .lock()
        .unwrap()
        .update_note(
            shared,
            NoteChange {
                body: Some("booked by alex".into()),
                ..NoteChange::default()
            },
        )
        .unwrap();
    let (phone, _) = round(phone).await;
    let (laptop, _) = round(laptop).await;
    assert!(view_of(&laptop).contains("booked by alex"));
    assert_eq!(space_view(&laptop, &space), space_view(&phone, &space));

    // read only now: alex's device keeps it but cannot change it
    let (status, _) = call(
        &app,
        "PUT",
        &format!("/sync/spaces/{space}/members"),
        Some(token(&sam)),
        Some(json!({ "members": [{ "userId": user_id(&alex), "role": "read" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (phone, _) = round(phone).await;
    assert!(
        phone
            .0
            .lock()
            .unwrap()
            .update_note(
                shared,
                NoteChange {
                    body: Some("no".into()),
                    ..NoteChange::default()
                },
            )
            .is_err()
    );

    // stopping sharing: sam moves it back, alex loses it
    laptop
        .0
        .lock()
        .unwrap()
        .move_to_space(Item::Folder(moved), "")
        .unwrap();
    let (laptop, _) = round(laptop).await;
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/sync/spaces/{space}"),
        Some(token(&sam)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (laptop, _) = round(laptop).await;
    let (phone, _) = round(phone).await;
    assert!(!view_of(&phone).contains("booked by alex"));
    assert!(
        view_of(&laptop).contains("booked by alex"),
        "the owner keeps it"
    );
    assert!(space_view(&laptop, "").contains("Trip"));
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "one story, read top to bottom")]
async fn only_members_reach_a_space_and_only_the_owner_manages_it() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
    let kim = standard(&app, "kim").await;
    let (_, sam) = login(&app, "admin", "admin password").await;
    let space = share(
        &app,
        &sam,
        json!([{ "userId": user_id(&alex), "role": "read" }]),
    )
    .await;

    let push = json!({ "changes": [{ "id": "c1", "doc": "space", "data": "AAE=" }] });
    let (status, _) = call(
        &app,
        "POST",
        &format!("/sync/push?space={space}"),
        Some(token(&alex)),
        Some(push.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a reader cannot push");
    let (status, _) = call(
        &app,
        "GET",
        &format!("/sync/changes?space={space}"),
        Some(token(&alex)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "but reads");
    for (method, path) in [
        ("GET", format!("/sync/changes?space={space}")),
        ("GET", format!("/sync/heavy?space={space}")),
        ("GET", format!("/sync/spaces/{space}/members")),
        ("POST", format!("/sync/push?space={space}")),
    ] {
        let (status, _) = call(&app, method, &path, Some(token(&kim)), Some(push.clone())).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }
    let (_, listed) = call(&app, "GET", "/sync/spaces", Some(token(&kim)), None).await;
    assert_eq!(listed["spaces"], json!([]));

    let members = json!({ "members": [{ "userId": user_id(&kim), "role": "edit" }] });
    let (status, _) = call(
        &app,
        "PUT",
        &format!("/sync/spaces/{space}/members"),
        Some(token(&alex)),
        Some(members.clone()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "members do not share further"
    );
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/sync/spaces/{space}"),
        Some(token(&alex)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for bad in [
        json!([{ "userId": user_id(&sam), "role": "edit" }]),
        json!([{ "userId": "nobody", "role": "edit" }]),
        json!([{ "userId": user_id(&kim), "role": "owner" }]),
        json!([{ "role": "edit" }]),
        json!([{ "groupId": "nothing", "role": "read" }]),
    ] {
        let (status, body) = call(
            &app,
            "PUT",
            &format!("/sync/spaces/{space}/members"),
            Some(token(&sam)),
            Some(json!({ "members": bad })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} {body}");
    }

    // alex leaves, the owner cannot
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/sync/spaces/{space}/me"),
        Some(token(&alex)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, listed) = call(&app, "GET", "/sync/spaces", Some(token(&alex)), None).await;
    assert_eq!(listed["spaces"], json!([]));
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/sync/spaces/{space}/me"),
        Some(token(&sam)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // the member list, for everyone in it
    let (status, list) = call(
        &app,
        "PUT",
        &format!("/sync/spaces/{space}/members"),
        Some(token(&sam)),
        Some(members),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let names: Vec<&str> = list["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["admin", "kim"]);

    // people are found by their exact name only
    let (status, found) = call(
        &app,
        "GET",
        "/sync/people?name=Kim",
        Some(token(&alex)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["userId"], user_id(&kim));
    let (status, _) = call(
        &app,
        "GET",
        "/sync/people?name=ki",
        Some(token(&alex)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn group_members_are_in_while_in_the_group_and_can_leave_on_their_own() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
    let (_, group) = call(
        &app,
        "POST",
        "/admin/groups",
        Some(&admin),
        Some(json!({ "name": "Family" })),
    )
    .await;
    let group = group["id"].as_str().unwrap().to_owned();
    let space = share(
        &app,
        &json!({ "accessToken": admin }),
        json!([{ "groupId": group, "role": "edit" }]),
    )
    .await;
    let listed = |token: String| {
        let app = app.clone();
        async move {
            let (_, listed) = call(&app, "GET", "/sync/spaces", Some(&token), None).await;
            listed["spaces"].as_array().unwrap().len()
        }
    };
    assert_eq!(listed(token(&alex).into()).await, 0);
    call(
        &app,
        "PUT",
        &format!("/admin/groups/{group}/members/{}", user_id(&alex)),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(listed(token(&alex).into()).await, 1, "in through the group");
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/sync/spaces/{space}/me"),
        Some(token(&alex)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        listed(token(&alex).into()).await,
        0,
        "left, the group stays"
    );
    let (_, members) = call(
        &app,
        "GET",
        &format!("/sync/spaces/{space}/members"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(members["members"][1]["name"], "Family");
}

/// Sealed bytes of a space as the server keeps them.
async fn stored(state: &AppState, space: &str) -> Vec<Vec<u8>> {
    let space = space.to_owned();
    state
        .db
        .call(move |c| {
            let mut stmt = c.prepare("SELECT bytes FROM sync_changes WHERE space_id = ?1")?;
            let rows = stmt
                .query_map([space], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<Vec<u8>>>>()?;
            Ok(rows)
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[expect(clippy::too_many_lines, reason = "one story, read top to bottom")]
async fn e2ee_accounts_share_with_keys_the_server_never_holds() {
    let (state, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let (zoe, laptop) = e2ee(&app, "zoe").await;
    let (yan, phone) = e2ee(&app, "yan").await;
    let (kai, tablet) = e2ee(&app, "kai").await;
    let bob = standard(&app, "bob").await;

    // the first key pair stays
    let other = app_e2ee::new_key_pair();
    let (_, kept) = call(
        &app,
        "PUT",
        "/sync/key-pair",
        Some(token(&zoe)),
        Some(json!({ "publicKey": other.public_text(), "privateSealed": "x" })),
    )
    .await;
    assert_ne!(kept["publicKey"], other.public_text());
    let (status, _) = call(
        &app,
        "PUT",
        "/sync/key-pair",
        Some(token(&bob)),
        Some(json!({ "publicKey": other.public_text(), "privateSealed": "x" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "standard accounts have none"
    );

    let (status, _) = call(
        &app,
        "POST",
        "/sync/spaces",
        Some(token(&zoe)),
        Some(json!({ "members": [{ "userId": user_id(&bob), "role": "edit" }] })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "e2ee notes go to e2ee accounts only"
    );
    let space = share(
        &app,
        &zoe,
        json!([{ "userId": user_id(&yan), "role": "edit" }]),
    )
    .await;
    let (laptop, _) = round(laptop).await;
    let id = note(&laptop, "Plan", "the secret plan");
    laptop
        .0
        .lock()
        .unwrap()
        .move_to_space(Item::Note(id), &space)
        .unwrap();
    let (laptop, _) = round(laptop).await;
    let (phone, came) = round(phone).await;
    assert_eq!(came.taken.rejected, 0, "{came:?}");
    assert!(view_of(&phone).contains("the secret plan"));
    assert_eq!(space_view(&laptop, &space), space_view(&phone, &space));
    for bytes in stored(&state, &space).await {
        assert!(!bytes.windows(6).any(|w| w == b"secret"), "sealed");
    }

    // yan is removed: a new key, kai who joins reads everything, yan nothing new
    let (status, _) = call(
        &app,
        "PUT",
        &format!("/sync/spaces/{space}/members"),
        Some(token(&zoe)),
        Some(json!({ "members": [{ "userId": user_id(&kai), "role": "read" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, listed) = call(&app, "GET", "/sync/spaces", Some(token(&zoe)), None).await;
    assert_eq!(listed["spaces"][0]["epoch"], 2);
    let (laptop, _) = round(laptop).await;
    let shared = laptop
        .0
        .lock()
        .unwrap()
        .list_notes(
            app_core::model::NoteScope::All,
            app_core::model::NoteSort::TitleAsc,
        )
        .unwrap()[0]
        .id;
    laptop
        .0
        .lock()
        .unwrap()
        .update_note(
            shared,
            NoteChange {
                body: Some("the new plan".into()),
                ..NoteChange::default()
            },
        )
        .unwrap();
    let (laptop, _) = round(laptop).await;
    let (tablet, came) = round(tablet).await;
    assert_eq!(came.taken.rejected, 0, "{came:?}");
    assert!(view_of(&tablet).contains("the new plan"));
    assert_eq!(space_view(&laptop, &space), space_view(&tablet, &space));
    assert_eq!(
        tablet.0.lock().unwrap().space_key_epochs(&space).unwrap(),
        vec![1, 2]
    );
    let (phone, _) = round(phone).await;
    assert!(!view_of(&phone).contains("plan"), "yan lost it");
    let (_, listed) = call(&app, "GET", "/sync/spaces", Some(token(&yan)), None).await;
    assert_eq!(listed["spaces"], json!([]));
    drop(phone);

    // a key only the owner's devices give, for members only
    let (status, _) = call(
        &app,
        "POST",
        &format!("/sync/spaces/{space}/grants"),
        Some(token(&kai)),
        Some(json!({ "epoch": 2, "grants": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        "POST",
        &format!("/sync/spaces/{space}/grants"),
        Some(token(&zoe)),
        Some(json!({ "epoch": 2, "grants": [{ "userId": user_id(&yan), "grant": "x" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &app,
        "POST",
        &format!("/sync/spaces/{space}/grants"),
        Some(token(&zoe)),
        Some(json!({ "epoch": 3, "grants": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    drop((laptop, tablet));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_s_live_socket_hears_of_the_shared_space() {
    use super::sync_tests::{live_socket, next_message};
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
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
    let wait = std::time::Duration::from_secs(2);
    let mut socket = live_socket(addr, Some(token(&alex))).await.unwrap();
    next_message(&mut socket, wait).await.unwrap().unwrap();

    let space = share(
        &app,
        &json!({ "accessToken": admin }),
        json!([{ "userId": user_id(&alex), "role": "edit" }]),
    )
    .await;
    let joined = next_message(&mut socket, wait).await.unwrap().unwrap();
    assert_eq!(
        joined,
        json!({ "type": "changed", "head": 0 }),
        "added: sync"
    );
    let (status, _) = call(
        &app,
        "POST",
        &format!("/sync/push?space={space}"),
        Some(&admin),
        Some(json!({ "changes": [{ "id": "c1", "doc": "space", "data": "AAE=" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pushed = next_message(&mut socket, wait).await.unwrap().unwrap();
    assert_eq!(pushed["space"], space.as_str());
    assert!(pushed["head"].as_i64().unwrap() > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn leaving_a_group_starts_a_new_key_in_its_e2ee_spaces() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let (zoe, laptop) = e2ee(&app, "zoe").await;
    let (yan, phone) = e2ee(&app, "yan").await;
    let (_, group) = call(
        &app,
        "POST",
        "/admin/groups",
        Some(&admin),
        Some(json!({ "name": "Team" })),
    )
    .await;
    let group = group["id"].as_str().unwrap().to_owned();
    let member = format!("/admin/groups/{group}/members/{}", user_id(&yan));
    call(&app, "PUT", &member, Some(&admin), None).await;
    // zoe shares with her own group
    let zoe_in = format!("/admin/groups/{group}/members/{}", user_id(&zoe));
    call(&app, "PUT", &zoe_in, Some(&admin), None).await;
    let space = share(&app, &zoe, json!([{ "groupId": group, "role": "edit" }])).await;
    let (laptop, _) = round(laptop).await;
    let (phone, _) = round(phone).await;
    assert_eq!(
        phone.0.lock().unwrap().space_key_epochs(&space).unwrap(),
        vec![1],
        "a key through the group"
    );
    let epoch = |app: Router, token: String| async move {
        let (_, listed) = call(&app, "GET", "/sync/spaces", Some(&token), None).await;
        listed["spaces"][0]["epoch"].as_u64().unwrap()
    };
    assert_eq!(epoch(app.clone(), token(&zoe).into()).await, 1);
    let (status, _) = call(&app, "DELETE", &member, Some(&admin), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(epoch(app.clone(), token(&zoe).into()).await, 2, "a new key");
    let (laptop, _) = round(laptop).await;
    assert_eq!(
        laptop.0.lock().unwrap().space_key_epochs(&space).unwrap(),
        vec![1, 2]
    );
    drop((laptop, phone));
}

#[tokio::test(flavor = "multi_thread")]
async fn members_see_who_views_a_note_and_strangers_cannot_say_they_do() {
    use super::sync_tests::{live_socket, next_message};
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
    let kim = standard(&app, "kim").await;
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
    let wait = std::time::Duration::from_secs(2);
    let quiet = std::time::Duration::from_millis(400);
    let space = share(
        &app,
        &json!({ "accessToken": admin }),
        json!([{ "userId": user_id(&alex), "role": "edit" }]),
    )
    .await;
    let mut sam = live_socket(addr, Some(&admin)).await.unwrap();
    next_message(&mut sam, wait).await.unwrap().unwrap();
    let mut phone = live_socket(addr, Some(token(&alex))).await.unwrap();
    next_message(&mut phone, wait).await.unwrap().unwrap();
    let mut stranger = live_socket(addr, Some(token(&kim))).await.unwrap();
    next_message(&mut stranger, wait).await.unwrap().unwrap();

    let note = "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
    let viewing = json!({ "type": "viewing", "space": space, "note": note }).to_string();
    stranger
        .send(Message::Text(viewing.clone().into()))
        .await
        .unwrap();
    assert!(
        next_message(&mut sam, quiet).await.is_none(),
        "kim is not in it"
    );

    phone.send(Message::Text(viewing.into())).await.unwrap();
    let seen = next_message(&mut sam, wait).await.unwrap().unwrap();
    assert_eq!(
        seen,
        json!({ "type": "presence", "space": space, "viewers": [{ "name": "alex", "displayName": "alex", "note": note }] })
    );
    let own = next_message(&mut phone, wait).await.unwrap().unwrap();
    assert_eq!(own["type"], "presence", "alex hears it too");

    // Taken out of the space, alex stops
    // showing in the note
    let (status, _) = call(
        &app,
        "PUT",
        &format!("/sync/spaces/{space}/members"),
        Some(&admin),
        Some(json!({ "members": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut left = false;
    while let Some(Ok(message)) = next_message(&mut sam, wait).await {
        if message["type"] == "presence" && message["viewers"] == json!([]) {
            left = true;
            break;
        }
    }
    assert!(left, "the others no longer see alex in the note");

    // and a socket that floods "viewing" messages is closed
    for _ in 0..40 {
        if stranger
            .send(Message::Text(
                json!({ "type": "viewing" }).to_string().into(),
            ))
            .await
            .is_err()
        {
            break;
        }
    }
    let mut closed = false;
    while let Some(message) = next_message(&mut stranger, wait).await {
        if message.is_err() {
            closed = true;
            break;
        }
    }
    assert!(closed, "flooding closes the socket");
    drop(phone);
}

#[tokio::test]
async fn a_deleted_account_takes_its_shares_or_its_place_in_them() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
    let kim = standard(&app, "kim").await;
    let space = share(
        &app,
        &alex,
        json!([{ "userId": user_id(&kim), "role": "edit" }]),
    )
    .await;
    let mine = share(
        &app,
        &json!({ "accessToken": admin }),
        json!([{ "userId": user_id(&kim), "role": "read" }, { "userId": user_id(&alex), "role": "edit" }]),
    )
    .await;
    let push = json!({ "changes": [{ "id": "c1", "doc": "space", "data": "AAE=" }] });
    let (status, _) = call(
        &app,
        "POST",
        &format!("/sync/push?space={space}"),
        Some(token(&alex)),
        Some(push),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // the owner goes: the space and its changes with them
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/admin/users/{}", user_id(&alex)),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, listed) = call(&app, "GET", "/sync/spaces", Some(token(&kim)), None).await;
    let ids: Vec<&str> = listed["spaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [mine.as_str()], "only the admin's share is left");
    let (status, _) = call(
        &app,
        "GET",
        &format!("/sync/changes?space={space}"),
        Some(token(&kim)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // a member goes: the others no longer see them in it
    let (_, members) = call(
        &app,
        "GET",
        &format!("/sync/spaces/{mine}/members"),
        Some(&admin),
        None,
    )
    .await;
    let names: Vec<&str> = members["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["admin", "kim"]);
}

fn first_note(device: &Device) -> app_core::id::NoteId {
    device
        .0
        .lock()
        .unwrap()
        .list_notes(
            app_core::model::NoteScope::All,
            app_core::model::NoteSort::TitleAsc,
        )
        .unwrap()[0]
        .id
}

fn folder_of(device: &Device, notes: usize, body: &str) -> app_core::id::FolderId {
    let mut store = device.0.lock().unwrap();
    let folder = store.create_folder(None, "Big").unwrap();
    for i in 0..notes {
        store
            .create_note(&NewNote {
                title: format!("Note {i}"),
                body: body.to_owned(),
                folder_id: Some(folder.id),
                ..NewNote::default()
            })
            .unwrap();
    }
    folder.id
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement, run by hand"]
#[expect(clippy::too_many_lines, reason = "one measurement, read top to bottom")]
async fn sharing_costs_through_the_server() {
    use std::time::Instant;
    // twenty sign-ups in a few seconds, more than the default limit lets in
    let config = crate::config::Config {
        auth: crate::config::AuthConfig {
            rate_limit_per_minute: 1000,
            ..crate::config::Config::default().auth
        },
        ..crate::config::Config::default()
    };
    let (_, app, admin) = super::tests::admin_on(super::tests::server_with(config).await).await;
    open_registration(&app, &admin).await;
    let body = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(18);

    // standard: 2000 notes of 1 KB shared with one person
    let alex = standard(&app, "alex").await;
    let (_, sam) = login(&app, "admin", "admin password").await;
    let laptop = linked(&app, &sam, None);
    let folder = folder_of(&laptop, 2000, &body);
    let (laptop, _) = round(laptop).await;
    let space = share(
        &app,
        &sam,
        json!([{ "userId": user_id(&alex), "role": "edit" }]),
    )
    .await;
    let (laptop, _) = round(laptop).await;
    let t = Instant::now();
    let moved: app_core::id::FolderId = laptop
        .0
        .lock()
        .unwrap()
        .move_to_space(Item::Folder(folder), &space)
        .unwrap()
        .parse()
        .unwrap();
    println!("move 2000 notes into the share: {:?}", t.elapsed());
    let t = Instant::now();
    let (laptop, pushed) = round(laptop).await;
    println!(
        "push the move: {:?}, {} changes",
        t.elapsed(),
        pushed.pushed
    );
    let phone = linked(&app, &alex, None);
    let t = Instant::now();
    let (phone, took) = round(phone).await;
    println!(
        "the member's first round takes 2000 notes: {:?}, {} applied",
        t.elapsed(),
        took.taken.applied
    );
    assert_eq!(space_view(&laptop, &space), space_view(&phone, &space));

    let id = first_note(&phone);
    phone
        .0
        .lock()
        .unwrap()
        .update_note(
            id,
            NoteChange {
                body: Some(format!("{body} edited by alex")),
                ..NoteChange::default()
            },
        )
        .unwrap();
    let t = Instant::now();
    let (_phone, _) = round(phone).await;
    let push_one = t.elapsed();
    let t = Instant::now();
    let (laptop, _) = round(laptop).await;
    println!(
        "one edit by the member: push {push_one:?}, the owner's round {:?}",
        t.elapsed()
    );
    let t = Instant::now();
    laptop
        .0
        .lock()
        .unwrap()
        .move_to_space(Item::Folder(moved), "")
        .unwrap();
    println!("stop sharing, 2000 notes back: {:?}", t.elapsed());

    // e2ee: 500 notes shared with 20 people
    let (zoe, owner) = e2ee(&app, "zoe").await;
    let mut members = Vec::new();
    let mut first = None;
    for i in 0..20 {
        let (session, device) = e2ee(&app, &format!("member{i}")).await;
        members.push(json!({ "userId": user_id(&session), "role": "read" }));
        if first.is_none() {
            first = Some(device);
        }
    }
    let folder = folder_of(&owner, 500, &body);
    let (owner, _) = round(owner).await;
    let space = share(&app, &zoe, json!(members)).await;
    let t = Instant::now();
    let (owner, _) = round(owner).await;
    println!("the owner seals the key for 20 people: {:?}", t.elapsed());
    let t = Instant::now();
    owner
        .0
        .lock()
        .unwrap()
        .move_to_space(Item::Folder(folder), &space)
        .unwrap();
    let (owner, pushed) = round(owner).await;
    println!(
        "move and push 500 e2ee notes: {:?}, {} changes",
        t.elapsed(),
        pushed.pushed
    );
    let t = Instant::now();
    let (member, took) = round(first.unwrap()).await;
    println!(
        "a member opens the key and takes 500 notes: {:?}, {} applied",
        t.elapsed(),
        took.taken.applied
    );
    assert_eq!(space_view(&owner, &space), space_view(&member, &space));
}

#[tokio::test]
async fn a_group_gets_shares_only_from_its_members_or_an_admin() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
    let sam = standard(&app, "sam").await;
    let (_, group) = call(
        &app,
        "POST",
        "/admin/groups",
        Some(&admin),
        Some(json!({ "name": "Family" })),
    )
    .await;
    let group = group["id"].as_str().unwrap().to_owned();
    call(
        &app,
        "PUT",
        &format!("/admin/groups/{group}/members/{}", user_id(&sam)),
        Some(&admin),
        None,
    )
    .await;
    let members = json!([{ "groupId": group, "role": "edit" }]);

    // alex is not in the group: refused, nothing reaches sam
    let (status, answer) = call(
        &app,
        "POST",
        "/sync/spaces",
        Some(token(&alex)),
        Some(json!({ "members": members })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    let (_, listed) = call(&app, "GET", "/sync/spaces", Some(token(&sam)), None).await;
    assert_eq!(listed["spaces"].as_array().unwrap().len(), 0);

    // a member of the group, and an admin who is not, may
    share(&app, &sam, members.clone()).await;
    share(&app, &json!({ "accessToken": admin }), members).await;
}

#[tokio::test]
async fn a_member_holds_ten_unused_invites_at_most() {
    let (_, app, admin) = with_admin().await;
    open_registration(&app, &admin).await;
    let alex = standard(&app, "alex").await;
    call(
        &app,
        "PUT",
        "/admin/registration",
        Some(&admin),
        Some(json!({ "registration": "invite" })),
    )
    .await;
    let mut ids = Vec::new();
    for _ in 0..10 {
        let (status, invite) = call(
            &app,
            "POST",
            "/invites",
            Some(token(&alex)),
            Some(json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{invite}");
        ids.push(invite["id"].as_str().unwrap().to_owned());
    }
    let (status, answer) = call(
        &app,
        "POST",
        "/invites",
        Some(token(&alex)),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    // revoking one makes room, and admins have no cap
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/invites/{}", ids[0]),
        Some(token(&alex)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        "POST",
        "/invites",
        Some(token(&alex)),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    for _ in 0..11 {
        let (status, _) = call(&app, "POST", "/invites", Some(&admin), Some(json!({}))).await;
        assert_eq!(status, StatusCode::CREATED);
    }
}
