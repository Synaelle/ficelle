// SPDX-License-Identifier: AGPL-3.0-or-later

//! Devices syncing directly: each says its versions, the other answers with
//! what it lacks. No network, the stores talk in the same process.

use std::sync::Arc;

use crate::clock::{MINUTE, ManualClock};
use crate::id::NoteId;
use crate::model::{NoteScope, NoteSort};
use crate::store::{NewLink, NewNote, NoteChange, PeerDevice, Store, Tokens};
use crate::testing::{Rng, random_change, view};
use app_protocol::e2ee::AccountKind;

fn device(start: i64) -> (Store, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(start));
    (
        Store::with_clock(crate::db::open_in_memory(), clock.clone()),
        clock,
    )
}

/// What `from` has and `to` lacks goes to `to`. Says how many documents
/// changed there.
fn send(from: &mut Store, to: &mut Store) -> u32 {
    let versions = to.peer_versions().unwrap();
    let changes = from.peer_changes(&versions).unwrap();
    to.peer_take(&changes).unwrap()
}

/// Both ways, as one exchange does.
fn meet(a: &mut Store, b: &mut Store) {
    send(a, b);
    send(b, a);
    assert!(
        a.peer_changes(&b.peer_versions().unwrap())
            .unwrap()
            .is_empty()
            && b.peer_changes(&a.peer_versions().unwrap())
                .unwrap()
                .is_empty(),
        "something left to send after meeting"
    );
}

fn same(a: &Store, b: &Store) {
    let (va, vb) = (view(a), view(b));
    assert_eq!(va, vb, "devices differ\n--- a\n{va}\n--- b\n{vb}");
}

fn note_with(s: &mut Store, title: &str, body: &str) -> NoteId {
    s.create_note(&NewNote {
        title: title.into(),
        body: body.into(),
        ..NewNote::default()
    })
    .unwrap()
    .id
}

fn edit_body(s: &mut Store, id: NoteId, body: &str) {
    s.update_note(
        id,
        NoteChange {
            body: Some(body.into()),
            ..NoteChange::default()
        },
    )
    .unwrap();
}

#[test]
fn notes_tags_and_pins_reach_the_other_device_and_nothing_echoes() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let note = a
        .create_note(&NewNote {
            title: "Tomatoes".into(),
            body: "Six cedar posts".into(),
            tags: vec!["garden/summer".into(), "todo".into()],
            pinned: true,
            folder_id: None,
        })
        .unwrap();
    let outbox = b.sync_outbox(usize::MAX).unwrap().len();
    meet(&mut a, &mut b);
    let on_b = b.note(note.id).unwrap();
    assert_eq!(on_b.title, "Tomatoes");
    assert!(on_b.pinned);
    assert_eq!(b.search("cedar", 10).unwrap().len(), 1);
    same(&a, &b);
    assert_eq!(
        b.sync_outbox(usize::MAX).unwrap().len(),
        outbox,
        "taking changes in queued nothing of its own"
    );
    // meeting again sends nothing
    assert_eq!(send(&mut a, &mut b), 0);
    assert_eq!(send(&mut b, &mut a), 0);
}

#[test]
fn two_vaults_with_their_own_notes_come_together() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(2_000_000);
    note_with(&mut a, "On the desktop", "one");
    note_with(&mut b, "On the phone", "two");
    meet(&mut a, &mut b);
    same(&a, &b);
    assert_eq!(a.counts().unwrap().0, 2);
}

#[test]
fn edits_on_both_sides_merge() {
    let (mut a, ca) = device(1_000_000);
    let (mut b, cb) = device(1_000_000);
    let id = note_with(&mut a, "Plan", "first line\nsecond line\nthird line");
    meet(&mut a, &mut b);
    ca.advance(MINUTE);
    cb.advance(MINUTE);
    edit_body(&mut a, id, "FIRST line\nsecond line\nthird line");
    edit_body(
        &mut b,
        id,
        "first line\nsecond line\nthird line, edited on b",
    );
    meet(&mut a, &mut b);
    assert_eq!(
        a.note(id).unwrap().body,
        "FIRST line\nsecond line\nthird line, edited on b"
    );
    same(&a, &b);
}

#[test]
fn rewrites_of_the_same_paragraph_ask_on_both_devices() {
    const BASE: &str =
        "# Trip\n\nWe leave on Monday morning by train from the north station.\n\nPack light.";
    const MINE: &str =
        "# Trip\n\nPlans changed: we drive down on Sunday night with Alex.\n\nPack light.";
    const THEIRS: &str =
        "# Trip\n\nFlights booked, the plane leaves Friday at noon.\n\nPack light.";
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let id = note_with(&mut a, "Trip", BASE);
    meet(&mut a, &mut b);
    edit_body(&mut a, id, MINE);
    edit_body(&mut b, id, THEIRS);
    meet(&mut a, &mut b);
    same(&a, &b);
    let on_a = a.note_conflict(id).unwrap().expect("a asks");
    assert_eq!(on_a.kind, "merge");
    assert_eq!(on_a.mine.unwrap().body, MINE);
    let on_b = b.note_conflict(id).unwrap().expect("b asks");
    assert_eq!(on_b.mine.unwrap().body, THEIRS);
}

#[test]
fn a_note_deleted_on_one_device_and_edited_on_the_other_comes_back() {
    let (mut a, ca) = device(1_000_000);
    let (mut b, cb) = device(1_000_000);
    let id = note_with(&mut a, "Keys", "under the mat");
    meet(&mut a, &mut b);
    ca.advance(MINUTE);
    cb.advance(MINUTE);
    a.trash_note(id).unwrap();
    a.purge_note(id).unwrap();
    edit_body(&mut b, id, "under the mat, and a spare at Léa's");
    meet(&mut a, &mut b);
    same(&a, &b);
    assert!(a.note(id).unwrap().body.contains("spare"));
}

#[test]
fn files_a_device_lacks_are_listed_with_their_hash() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let id = note_with(&mut a, "Receipt", "the scan");
    let sha = crate::blobs::sha256_hex(b"receipt bytes");
    a.add_attachment(id, "receipt.pdf", "application/pdf", 13, &sha)
        .unwrap();
    meet(&mut a, &mut b);
    let wanted: Vec<_> = b.files_in_use().unwrap();
    assert!(wanted.iter().any(|f| f.sha256 == sha && f.size == Some(13)));
}

#[test]
fn a_cut_exchange_goes_on_from_where_it_stood() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    for i in 0..12 {
        note_with(&mut a, &format!("Note {i}"), "text");
    }
    let changes = a.peer_changes(&b.peer_versions().unwrap()).unwrap();
    // only the first batch arrives
    b.peer_take(&changes[..5]).unwrap();
    meet(&mut a, &mut b);
    same(&a, &b);
}

#[test]
fn changes_outside_the_personal_space_or_unreadable_are_refused_whole() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    note_with(&mut a, "Kept", "x");
    let mut changes = a.peer_changes(&b.peer_versions().unwrap()).unwrap();
    changes.push(("shared/space".into(), vec![1, 2, 3]));
    assert!(b.peer_take(&changes).is_err());
    changes.pop();
    changes.push(("space".into(), vec![9, 9, 9]));
    assert!(b.peer_take(&changes).is_err());
    assert_eq!(b.counts().unwrap().0, 0, "nothing was taken");
    changes.pop();
    b.peer_take(&changes).unwrap();
    assert_eq!(b.counts().unwrap().0, 1);
}

#[test]
fn random_devices_meeting_in_pairs_end_the_same() {
    let runs: u64 = std::env::var("SYNC_FUZZ_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    for seed in 1..=runs {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let clocks: Vec<_> = (0..3)
            .map(|i| Arc::new(ManualClock::new(1_000_000 + i * 7_919)))
            .collect();
        let mut devices: Vec<Store> = clocks
            .iter()
            .map(|c| Store::with_clock(crate::db::open_in_memory(), c.clone()))
            .collect();
        for _ in 0..80 {
            let d = rng.below(3);
            clocks[d].advance(i64::try_from(rng.below(5) + 1).unwrap() * 61_000);
            random_change(&mut devices[d], &mut rng);
            if rng.below(4) == 0 {
                let other = (d + 1 + rng.below(2)) % 3;
                let (x, y) = pair(&mut devices, d, other);
                if rng.below(2) == 0 {
                    send(x, y);
                } else {
                    meet(x, y);
                }
            }
        }
        // everyone meets the one in the middle, twice round
        for _ in 0..2 {
            for other in [0, 2] {
                let (x, y) = pair(&mut devices, 1, other);
                meet(x, y);
            }
        }
        let views: Vec<String> = devices.iter().map(view).collect();
        for d in 1..3 {
            assert_eq!(
                views[0], views[d],
                "seed {seed}: device {d} differs\n--- 0\n{}\n--- {d}\n{}",
                views[0], views[d]
            );
        }
    }
}

fn pair(devices: &mut [Store], x: usize, y: usize) -> (&mut Store, &mut Store) {
    assert_ne!(x, y);
    if x < y {
        let (low, high) = devices.split_at_mut(y);
        (&mut low[x], &mut high[0])
    } else {
        let (low, high) = devices.split_at_mut(x);
        (&mut high[0], &mut low[y])
    }
}

fn link() -> NewLink {
    NewLink {
        server: "https://notes.example.org".into(),
        account_id: "account".into(),
        username: "sam".into(),
        kind: AccountKind::Standard,
        device_id: "device".into(),
        tokens: Tokens {
            access: "a".into(),
            access_expires_at: 0,
            refresh: "r".into(),
            refresh_expires_at: 0,
        },
        master_key: None,
    }
}

#[test]
fn a_vault_syncs_with_a_server_or_with_its_devices_never_both() {
    let (mut s, _) = device(1_000_000);
    s.peer_pair(&[7; 32], "Pixel 8", "android", "0.6.0")
        .unwrap();
    assert!(s.devices_paired().unwrap());
    let refused = s.sync_set_link(&link()).unwrap_err();
    assert!(refused.to_string().contains(crate::store::DEVICES_PAIRED));
    s.peer_forget(&[7; 32]).unwrap();
    assert!(!s.devices_paired().unwrap());
    s.sync_set_link(&link()).unwrap();
    let refused = s
        .peer_pair(&[8; 32], "Laptop", "linux", "0.6.0")
        .unwrap_err();
    assert!(refused.to_string().contains(crate::store::SERVER_LINKED));
    assert!(s.peer_learn(&[], &[1; 32]).is_err());
    s.sync_unlink().unwrap();
    s.peer_pair(&[8; 32], "Laptop", "linux", "0.6.0").unwrap();
}

#[test]
fn a_device_list_learned_from_another_device_keeps_the_later_of_pairing_and_forgetting() {
    let (mut a, ca) = device(1_000_000);
    let (mut b, cb) = device(1_000_000);
    let own_a = [1u8; 32];
    let own_b = [2u8; 32];
    a.peer_pair(&own_b, "Phone", "android", "0.6.0").unwrap();
    b.peer_pair(&own_a, "Desktop", "linux", "0.6.0").unwrap();
    ca.advance(MINUTE);
    a.peer_pair(&[3; 32], "Tablet", "android", "0.6.0").unwrap();
    // b learns of the tablet, never of itself
    assert!(b.peer_learn(&a.peer_devices().unwrap(), &own_b).unwrap());
    let known: Vec<PeerDevice> = b.peer_devices().unwrap();
    assert_eq!(known.len(), 2);
    assert!(known.iter().all(PeerDevice::active));
    // the tablet is forgotten on b, a hears of it and does not bring it back
    cb.advance(2 * MINUTE);
    b.peer_forget(&[3; 32]).unwrap();
    assert!(a.peer_learn(&b.peer_devices().unwrap(), &own_a).unwrap());
    let tablet = |s: &Store| {
        s.peer_devices()
            .unwrap()
            .into_iter()
            .find(|d| d.key == [3; 32])
            .unwrap()
    };
    assert!(!tablet(&a).active());
    assert!(!b.peer_learn(&a.peer_devices().unwrap(), &own_b).unwrap());
    // paired again later on a, it comes back on b
    ca.advance(5 * MINUTE);
    a.peer_pair(&[3; 32], "Tablet", "android", "0.6.1").unwrap();
    b.peer_learn(&a.peer_devices().unwrap(), &own_b).unwrap();
    assert!(tablet(&b).active());
}

#[test]
fn devices_moving_from_a_server_to_local_sync_keep_one_copy_of_each_note() {
    // the server's relay, as the server sync tests do it
    fn relay(from: &mut Store, to: &mut Store) {
        let out = from.sync_outbox(usize::MAX).unwrap();
        let page: Vec<(String, Vec<u8>)> = out
            .iter()
            .map(|item| (item.doc_id.clone(), item.bytes.clone()))
            .collect();
        to.apply_remote_page(&page).unwrap();
        if let Some(last) = out.last() {
            from.sync_ack(last.seq).unwrap();
        }
    }
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    a.sync_set_link(&link()).unwrap();
    b.sync_set_link(&link()).unwrap();
    let shared = note_with(&mut a, "Plan", "both have it");
    a.set_note_tags(shared, &["Projects/Garden".into()])
        .unwrap();
    note_with(&mut b, "Groceries", "from the phone");
    relay(&mut a, &mut b);
    relay(&mut b, &mut a);
    relay(&mut a, &mut b);
    same(&a, &b);

    // both leave the server, each edits on its own, then they pair
    a.sync_unlink().unwrap();
    b.sync_unlink().unwrap();
    edit_body(&mut a, shared, "both have it, edited on the computer");
    note_with(&mut b, "Call Inès", "made while apart");
    a.peer_pair(&[7; 32], "Phone", "android", "0.6.0").unwrap();
    b.peer_pair(&[8; 32], "Desktop", "linux", "0.6.0").unwrap();
    meet(&mut a, &mut b);
    same(&a, &b);

    let titles: Vec<String> = a
        .list_notes(NoteScope::All, NoteSort::TitleAsc)
        .unwrap()
        .into_iter()
        .map(|n| n.title)
        .collect();
    assert_eq!(
        titles,
        ["Call Inès", "Groceries", "Plan"],
        "a note came twice"
    );
    assert_eq!(
        b.note(shared).unwrap().body,
        "both have it, edited on the computer"
    );
}
