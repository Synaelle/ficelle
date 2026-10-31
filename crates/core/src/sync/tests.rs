// SPDX-License-Identifier: AGPL-3.0-or-later

//! Two (or three) devices in one process, with a relay in place of the
//! server that can deliver changes late, out of order or twice.

use std::sync::Arc;

use crate::clock::{DAY, MINUTE, ManualClock};
use crate::id::NoteId;
use crate::model::{NoteScope, NoteSort};
use crate::store::{NewNote, NoteChange, Store};
use crate::testing::{Rng, random_change, view};

fn device(start: i64) -> (Store, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(start));
    (
        Store::with_clock(crate::db::open_in_memory(), clock.clone()),
        clock,
    )
}

/// Everything in `from`'s outbox, applied on `to` in order, then acked.
fn send(from: &mut Store, to: &mut Store) {
    let out = from.sync_outbox(usize::MAX).unwrap();
    for item in &out {
        to.apply_remote(&item.doc_id, &item.bytes).unwrap();
    }
    if let Some(last) = out.last() {
        from.sync_ack(last.seq).unwrap();
    }
}

/// Both ways. Taking in changes must never produce changes to send back.
fn sync(a: &mut Store, b: &mut Store) {
    send(a, b);
    send(b, a);
    assert!(a.sync_outbox(1).unwrap().is_empty(), "a echoed b's changes");
    assert!(b.sync_outbox(1).unwrap().is_empty(), "b echoed a's changes");
}

/// A server in miniature: keeps every change, each device reads from where
/// it left off.
#[derive(Default)]
struct Hub {
    log: Vec<(String, Vec<u8>)>,
}

impl Hub {
    fn push(&mut self, from: &mut Store) {
        let out = from.sync_outbox(usize::MAX).unwrap();
        self.log
            .extend(out.iter().map(|o| (o.doc_id.clone(), o.bytes.clone())));
        if let Some(last) = out.last() {
            from.sync_ack(last.seq).unwrap();
        }
    }

    fn pull(&self, to: &mut Store, cursor: &mut usize) {
        for (doc, bytes) in &self.log[*cursor..] {
            to.apply_remote(doc, bytes).unwrap();
        }
        *cursor = self.log.len();
    }
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
fn a_note_and_its_folder_tags_and_pin_reach_the_other_device() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let garden = a.create_folder(None, "Garden").unwrap();
    let note = a
        .create_note(&NewNote {
            folder_id: Some(garden.id),
            title: "Tomatoes".into(),
            body: "Six cedar posts".into(),
            tags: vec!["Summer".into(), "todo".into()],
            pinned: true,
        })
        .unwrap();
    sync(&mut a, &mut b);
    let on_b = b.note(note.id).unwrap();
    assert_eq!(on_b.title, "Tomatoes");
    assert_eq!(on_b.folder_id, Some(garden.id));
    assert_eq!(on_b.tags, vec!["Summer", "todo"]);
    assert!(on_b.pinned);
    same(&a, &b);
    // and search works on the other device, the index was rebuilt
    assert_eq!(b.search("cedar", 10).unwrap().len(), 1);
}

#[test]
fn edits_to_the_same_text_on_two_devices_merge() {
    let (mut a, ca) = device(1_000_000);
    let (mut b, cb) = device(1_000_000);
    let id = note_with(&mut a, "Plan", "first line\nsecond line\nthird line");
    sync(&mut a, &mut b);
    ca.advance(MINUTE);
    cb.advance(MINUTE);
    edit_body(&mut a, id, "FIRST line\nsecond line\nthird line");
    edit_body(
        &mut b,
        id,
        "first line\nsecond line\nthird line, edited on b",
    );
    a.update_note(
        id,
        NoteChange {
            title: Some("Plan for May".into()),
            ..NoteChange::default()
        },
    )
    .unwrap();
    sync(&mut a, &mut b);
    let note = a.note(id).unwrap();
    assert_eq!(
        note.body,
        "FIRST line\nsecond line\nthird line, edited on b"
    );
    assert_eq!(note.title, "Plan for May");
    same(&a, &b);
}

#[test]
fn the_text_before_a_remote_change_goes_to_the_history() {
    let (mut a, ca) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let id = note_with(&mut a, "Plan", "draft one");
    sync(&mut a, &mut b);
    ca.advance(30 * MINUTE);
    edit_body(&mut a, id, "draft two");
    sync(&mut a, &mut b);
    let versions = b.list_versions(id).unwrap();
    assert!(
        versions.iter().any(|v| v.body == "draft one"),
        "b keeps what it had: {versions:?}"
    );
}

#[test]
fn folders_moved_into_each_other_on_two_devices_do_not_loop() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let x = a.create_folder(None, "x").unwrap();
    let y = a.create_folder(None, "y").unwrap();
    let inside = note_with(&mut a, "inside x", "");
    a.move_note(inside, Some(x.id)).unwrap();
    sync(&mut a, &mut b);
    a.move_folder(x.id, Some(y.id)).unwrap();
    b.move_folder(y.id, Some(x.id)).unwrap();
    sync(&mut a, &mut b);
    same(&a, &b);
    let fx = a.folder(x.id).unwrap();
    let fy = a.folder(y.id).unwrap();
    assert!(
        !(fx.parent_id == Some(y.id) && fy.parent_id == Some(x.id)),
        "a loop: {fx:?} {fy:?}"
    );
    assert_eq!(b.note(inside).unwrap().folder_id, Some(x.id));
}

#[test]
fn folder_names_order_and_nesting_follow() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let p = a.create_folder(None, "Projects").unwrap();
    let one = a.create_folder(Some(p.id), "one").unwrap();
    let two = a.create_folder(Some(p.id), "two").unwrap();
    let three = a.create_folder(Some(p.id), "three").unwrap();
    sync(&mut a, &mut b);
    b.reorder_folders(Some(p.id), &[three.id, one.id, two.id])
        .unwrap();
    b.rename_folder(two.id, "deux").unwrap();
    sync(&mut a, &mut b);
    assert_eq!(a.folder(three.id).unwrap().position, 0);
    assert_eq!(a.folder(two.id).unwrap().name, "deux");
    same(&a, &b);
}

#[test]
fn a_note_trashed_on_one_device_and_edited_on_the_other_keeps_the_edit_in_the_trash() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let id = note_with(&mut a, "Shopping", "milk");
    sync(&mut a, &mut b);
    a.trash_note(id).unwrap();
    edit_body(&mut b, id, "milk, eggs");
    sync(&mut a, &mut b);
    let note = a.note(id).unwrap();
    assert!(note.trashed_at.is_some());
    assert_eq!(note.body, "milk, eggs");
    same(&a, &b);
}

#[test]
fn a_note_deleted_for_good_comes_back_if_the_other_device_edited_it() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let id = note_with(&mut a, "Shopping", "milk");
    a.trash_note(id).unwrap();
    sync(&mut a, &mut b);
    a.purge_note(id).unwrap();
    b.restore_note(id).unwrap();
    edit_body(&mut b, id, "milk, eggs, flour");
    sync(&mut a, &mut b);
    let back = a.note(id).expect("the edit brought the note back");
    assert_eq!(back.body, "milk, eggs, flour");
    assert!(back.trashed_at.is_some(), "back in the trash, not live");
    same(&a, &b);
}

#[test]
fn a_note_deleted_for_good_with_nothing_new_elsewhere_is_gone_everywhere() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let id = note_with(&mut a, "Old", "stale");
    a.set_note_tags(id, &["archive".into()]).unwrap();
    a.trash_note(id).unwrap();
    sync(&mut a, &mut b);
    a.purge_note(id).unwrap();
    sync(&mut a, &mut b);
    assert!(b.note(id).is_err());
    assert!(b.list_tags().unwrap().is_empty(), "its tag went with it");
    same(&a, &b);
}

#[test]
fn a_note_added_to_a_folder_trashed_elsewhere_follows_the_folder() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let folder = a.create_folder(None, "Trip").unwrap();
    sync(&mut a, &mut b);
    a.trash_folder(folder.id).unwrap();
    let late = b
        .create_note(&NewNote {
            folder_id: Some(folder.id),
            title: "Packing list".into(),
            ..NewNote::default()
        })
        .unwrap();
    sync(&mut a, &mut b);
    for s in [&a, &b] {
        let n = s.note(late.id).unwrap();
        assert!(n.trashed_at.is_some(), "in the trash with its folder");
        assert_eq!(n.folder_id, Some(folder.id));
    }
    same(&a, &b);
    a.restore_folder(folder.id).unwrap();
    sync(&mut a, &mut b);
    assert!(
        b.note(late.id).unwrap().trashed_at.is_none(),
        "back with the folder"
    );
    same(&a, &b);
}

#[test]
fn a_note_added_to_a_folder_deleted_elsewhere_survives_at_the_top() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let folder = a.create_folder(None, "Old").unwrap();
    a.trash_folder(folder.id).unwrap();
    sync(&mut a, &mut b);
    a.purge_folder(folder.id).unwrap();
    b.restore_folder(folder.id).unwrap();
    let late = b
        .create_note(&NewNote {
            folder_id: Some(folder.id),
            title: "Keep me".into(),
            ..NewNote::default()
        })
        .unwrap();
    sync(&mut a, &mut b);
    for s in [&a, &b] {
        let n = s.note(late.id).expect("the note survives");
        assert_eq!(n.title, "Keep me");
    }
    same(&a, &b);
}

#[test]
fn tags_added_on_two_devices_at_once_are_both_kept() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let id = note_with(&mut a, "Recipe", "");
    sync(&mut a, &mut b);
    a.set_note_tags(id, &["dinner".into()]).unwrap();
    b.set_note_tags(id, &["vegetarian".into()]).unwrap();
    sync(&mut a, &mut b);
    assert_eq!(a.note(id).unwrap().tags, vec!["dinner", "vegetarian"]);
    same(&a, &b);
    let tag = a
        .list_tags()
        .unwrap()
        .into_iter()
        .find(|t| t.name == "dinner")
        .unwrap();
    a.rename_tag(tag.id, "Supper").unwrap();
    sync(&mut a, &mut b);
    assert_eq!(b.note(id).unwrap().tags, vec!["Supper", "vegetarian"]);
    same(&a, &b);
}

#[test]
fn changes_late_out_of_order_and_twice_end_the_same() {
    let (mut a, ca) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let f = a.create_folder(None, "Inbox").unwrap();
    let id = note_with(&mut a, "Log", "");
    for i in 0..20 {
        ca.advance(MINUTE);
        let body = format!("{}line {i}\n", a.note(id).unwrap().body);
        edit_body(&mut a, id, &body);
    }
    a.move_note(id, Some(f.id)).unwrap();
    a.set_pinned(id, true).unwrap();
    let mut out = a.sync_outbox(usize::MAX).unwrap();
    // reversed, then every change once more
    out.reverse();
    let again = out.clone();
    out.extend(again);
    for item in &out {
        b.apply_remote(&item.doc_id, &item.bytes).unwrap();
    }
    a.sync_ack(i64::MAX).unwrap();
    assert!(b.sync_outbox(1).unwrap().is_empty());
    same(&a, &b);
}

#[test]
fn three_devices_offline_then_together() {
    let (mut a, ca) = device(1_000_000);
    let (mut b, cb) = device(1_000_000);
    let (mut c, cc) = device(1_000_000);
    let mut hub = Hub::default();
    let (mut kb, mut kc) = (0, 0);
    let id = note_with(&mut a, "Shared", "one\ntwo\nthree\n");
    let f = a.create_folder(None, "F").unwrap();
    hub.push(&mut a);
    hub.pull(&mut b, &mut kb);
    hub.pull(&mut c, &mut kc);
    let mut ka = hub.log.len();
    for clock in [&ca, &cb, &cc] {
        clock.advance(MINUTE);
    }
    // all three offline, each changes something
    edit_body(&mut a, id, "ONE\ntwo\nthree\n");
    edit_body(&mut b, id, "one\nTWO\nthree\n");
    edit_body(&mut c, id, "one\ntwo\nTHREE\n");
    b.move_note(id, Some(f.id)).unwrap();
    c.rename_folder(f.id, "Folder").unwrap();
    // back online one after the other
    hub.push(&mut c);
    hub.push(&mut a);
    hub.push(&mut b);
    hub.pull(&mut a, &mut ka);
    hub.pull(&mut b, &mut kb);
    hub.pull(&mut c, &mut kc);
    for s in [&mut a, &mut b, &mut c] {
        assert!(s.sync_outbox(1).unwrap().is_empty(), "no echo");
    }
    assert_eq!(a.note(id).unwrap().body, "ONE\nTWO\nTHREE\n");
    same(&a, &b);
    same(&b, &c);
}

#[test]
fn a_clock_years_ahead_does_not_break_anything() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000 + 3 * 365 * DAY);
    let id = note_with(&mut a, "Skew", "a");
    sync(&mut a, &mut b);
    edit_body(&mut b, id, "b");
    sync(&mut a, &mut b);
    edit_body(&mut a, id, "a again");
    sync(&mut a, &mut b);
    // order comes from the documents, not from wall clocks
    assert_eq!(b.note(id).unwrap().body, "a again");
    same(&a, &b);
}

#[test]
fn a_new_device_gets_everything_from_snapshots() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let f = a.create_folder(None, "Notes").unwrap();
    for i in 0..30 {
        let id = note_with(&mut a, &format!("n{i}"), &"text ".repeat(i));
        a.move_note(id, Some(f.id)).unwrap();
    }
    // a device starting from nothing: the space first, then each note
    let space = a.sync_snapshot(crate::sync::SPACE).unwrap();
    b.apply_remote(crate::sync::SPACE, &space).unwrap();
    let ids: Vec<_> = a
        .list_notes(NoteScope::All, NoteSort::TitleAsc)
        .unwrap()
        .into_iter()
        .map(|n| n.id)
        .collect();
    for id in ids {
        let snap = a.sync_snapshot(&id.to_string()).unwrap();
        b.apply_remote(&id.to_string(), &snap).unwrap();
    }
    a.sync_ack(i64::MAX).unwrap();
    assert!(b.sync_outbox(1).unwrap().is_empty());
    same(&a, &b);
}

#[test]
fn an_existing_vault_is_recorded_once_by_the_migration() {
    let (mut a, _) = device(1_000_000);
    let f = a.create_folder(None, "Kept").unwrap();
    let id = note_with(&mut a, "Old note", "from before sync");
    a.move_note(id, Some(f.id)).unwrap();
    a.set_note_tags(id, &["old".into()]).unwrap();
    // as if the vault predated sync: no documents, then the migration step
    a.wipe_sync_for_tests();
    a.bootstrap_sync_for_tests();
    let (mut b, _) = device(1_000_000);
    send(&mut a, &mut b);
    same(&a, &b);
    assert_eq!(b.note(id).unwrap().tags, vec!["old"]);
}

#[test]
fn recording_a_vault_again_changes_nothing() {
    // vaults built before the sync link step ran the recording already, the
    // migration that adds the link runs it again
    let (mut a, _) = device(1_000_000);
    let f = a.create_folder(None, "Kept").unwrap();
    let child = a.create_folder(Some(f.id), "Inner").unwrap();
    let id = note_with(&mut a, "Old note", "body");
    a.move_note(id, Some(child.id)).unwrap();
    a.set_note_tags(id, &["Old".into(), "two".into()]).unwrap();
    a.set_pinned(id, true).unwrap();
    let gone = note_with(&mut a, "gone", "");
    a.trash_note(gone).unwrap();
    let out = a.sync_outbox(usize::MAX).unwrap();
    a.sync_ack(out.last().unwrap().seq).unwrap();
    let before = view(&a);
    a.bootstrap_sync_for_tests();
    assert!(a.sync_outbox(1).unwrap().is_empty(), "recorded twice");
    assert_eq!(view(&a), before);
}

#[test]
fn blank_notes_discarded_do_not_come_back() {
    let (mut a, _) = device(1_000_000);
    let (mut b, _) = device(1_000_000);
    let blank = a.create_note(&NewNote::default()).unwrap();
    sync(&mut a, &mut b);
    assert!(a.discard_if_blank(blank.id).unwrap());
    sync(&mut a, &mut b);
    assert!(b.note(blank.id).is_err());
    same(&a, &b);
}

#[test]
fn change_names_never_repeat_even_after_the_outbox_empties() {
    let (mut a, _) = device(1_000_000);
    note_with(&mut a, "one", "");
    let first = a.sync_outbox(usize::MAX).unwrap();
    a.sync_ack(i64::MAX).unwrap();
    note_with(&mut a, "two", "");
    let second = a.sync_outbox(usize::MAX).unwrap();
    for change in &second {
        assert!(
            first.iter().all(|f| f.id != change.id),
            "{} was used before: the server would drop it",
            change.id
        );
    }
    let (mut b, _) = device(1_000_000);
    note_with(&mut b, "other", "");
    let other = b.sync_outbox(usize::MAX).unwrap();
    assert_ne!(
        other[0].id, first[0].id,
        "another vault names its changes apart"
    );
}

#[test]
fn many_edits_compact_and_reload_the_same() {
    let (mut a, ca) = device(1_000_000);
    let id = note_with(&mut a, "Long", "");
    for i in 0..200 {
        ca.advance(1000);
        let body = format!("{}{i} ", a.note(id).unwrap().body);
        edit_body(&mut a, id, &body);
    }
    let before = view(&a);
    // documents reload from the compacted snapshot plus what came after
    a.forget_docs_for_tests();
    let (mut b, _) = device(1_000_000);
    send(&mut a, &mut b);
    assert_eq!(view(&a), before);
    same(&a, &b);
    edit_body(&mut a, id, "after reload");
    send(&mut a, &mut b);
    assert_eq!(b.note(id).unwrap().body, "after reload");
}

/// Many random runs: three devices change things while offline for random
/// stretches, changes reach the others late, shuffled and sometimes twice.
/// In the end every device must show exactly the same, and none may have
/// anything left to send. `SYNC_FUZZ_RUNS` raises the count.
#[test]
fn random_devices_always_end_the_same() {
    let runs: u64 = std::env::var("SYNC_FUZZ_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let only: Option<u64> = std::env::var("SYNC_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok());
    for seed in 1..=runs {
        if only.is_some_and(|o| o != seed) {
            continue;
        }
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let clocks: Vec<_> = (0..3)
            .map(|i| Arc::new(ManualClock::new(1_000_000 + i * 7_919)))
            .collect();
        let mut devices: Vec<Store> = clocks
            .iter()
            .map(|c| Store::with_clock(crate::db::open_in_memory(), c.clone()))
            .collect();
        let mut hub = Hub::default();
        let mut cursors = [0usize; 3];
        for _ in 0..80 {
            let d = rng.below(3);
            clocks[d].advance(i64::try_from(rng.below(5) + 1).unwrap() * 61_000);
            random_change(&mut devices[d], &mut rng);
            if rng.below(4) == 0 {
                hub.push(&mut devices[d]);
            }
            if rng.below(5) == 0 {
                // deliver what is pending, shuffled, some of it twice
                let pending: Vec<_> = hub.log[cursors[d]..].to_vec();
                let mut order: Vec<usize> = (0..pending.len()).collect();
                for i in (1..order.len()).rev() {
                    order.swap(i, rng.below(i + 1));
                }
                for &i in &order {
                    devices[d]
                        .apply_remote(&pending[i].0, &pending[i].1)
                        .unwrap();
                    if rng.below(6) == 0 {
                        devices[d]
                            .apply_remote(&pending[i].0, &pending[i].1)
                            .unwrap();
                    }
                }
                cursors[d] = hub.log.len();
            }
        }
        for device in &mut devices {
            hub.push(device);
        }
        for (device, cursor) in devices.iter_mut().zip(cursors.iter_mut()) {
            hub.pull(device, cursor);
        }
        for (d, s) in devices.iter_mut().enumerate() {
            assert!(
                s.sync_outbox(1).unwrap().is_empty(),
                "seed {seed}: device {d} had changes left after taking everything in"
            );
        }
        if only.is_some() {
            for (d, s) in devices.iter_mut().enumerate() {
                println!("device {d}: {}", s.debug_space_tags_for_tests());
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

/// Timings, not a check: `cargo test --release -p app-core sync_costs -- --ignored --nocapture`.
#[test]
#[ignore = "timing run"]
fn sync_costs() {
    use std::time::Instant;
    let (mut a, clock) = device(1_000_000);
    let t = Instant::now();
    let mut ids = Vec::new();
    for i in 0..2000 {
        ids.push(note_with(
            &mut a,
            &format!("note {i}"),
            &"some text, ".repeat(90),
        ));
    }
    println!("create 2000 notes: {:?}", t.elapsed());
    let long = "a fairly long paragraph of markdown text. ".repeat(340);
    let id = note_with(&mut a, "Long", &long);
    let mut body = long.clone();
    let t = Instant::now();
    for i in 0..100 {
        clock.advance(1000);
        body.insert_str(body.len() / 2, &format!("{i}"));
        edit_body(&mut a, id, &body);
    }
    println!(
        "100 saves of a {} character note: {:?} each",
        body.len(),
        t.elapsed() / 100
    );
    let t = Instant::now();
    a.wipe_sync_for_tests();
    a.bootstrap_sync_for_tests();
    println!("migration of a vault of 2001 notes: {:?}", t.elapsed());
    let (mut b, _) = device(1_000_000);
    let t = Instant::now();
    let space = a.sync_snapshot(crate::sync::SPACE).unwrap();
    b.apply_remote(crate::sync::SPACE, &space).unwrap();
    println!(
        "a new device takes the space ({} KB): {:?}",
        space.len() / 1024,
        t.elapsed()
    );
    let t = Instant::now();
    for id in &ids {
        let snap = a.sync_snapshot(&id.to_string()).unwrap();
        b.apply_remote(&id.to_string(), &snap).unwrap();
    }
    println!("then 2000 notes one by one: {:?}", t.elapsed());
    a.sync_ack(i64::MAX).unwrap();
    let t = Instant::now();
    a.set_pinned(ids[0], true).unwrap();
    println!("pin a note among 2000: {:?}", t.elapsed());
    let out = a.sync_outbox(usize::MAX).unwrap();
    let t = Instant::now();
    for o in &out {
        b.apply_remote(&o.doc_id, &o.bytes).unwrap();
    }
    println!(
        "that pin reaching a device with 2000 notes: {:?} ({} changes)",
        t.elapsed(),
        out.len()
    );
}

mod conflicts {
    use super::*;
    use crate::model::VersionReason;
    use crate::store::Resolution;

    const BASE: &str =
        "# Trip\n\nWe leave on Monday morning by train from the north station.\n\nPack light.";
    const MINE: &str =
        "# Trip\n\nPlans changed: we drive down on Sunday night with Alex.\n\nPack light.";
    const THEIRS: &str =
        "# Trip\n\nFlights booked, the plane leaves Friday at noon.\n\nPack light.";

    /// Both devices rewrite the same paragraph while apart, then meet.
    fn rewritten_on_both() -> (Store, Store, NoteId) {
        let (mut a, _) = device(1_000_000);
        let (mut b, _) = device(1_000_000);
        let id = note_with(&mut a, "Trip", BASE);
        sync(&mut a, &mut b);
        edit_body(&mut a, id, MINE);
        edit_body(&mut b, id, THEIRS);
        sync(&mut a, &mut b);
        (a, b, id)
    }

    #[test]
    fn rewrites_of_the_same_paragraph_ask_on_both_devices_and_keep_both_in_history() {
        let (a, b, id) = rewritten_on_both();
        same(&a, &b);
        let merged = a.note(id).unwrap().body;
        assert!(
            merged.contains("Alex") && merged.contains("Friday"),
            "{merged}"
        );
        let on_a = a.note_conflict(id).unwrap().expect("a asks");
        assert_eq!(on_a.kind, "merge");
        assert_eq!(on_a.mine.unwrap().body, MINE);
        assert_eq!(on_a.theirs.unwrap().body, THEIRS);
        let on_b = b.note_conflict(id).unwrap().expect("b asks too");
        assert_eq!(on_b.mine.unwrap().body, THEIRS, "b's own text is its mine");
        let reasons: Vec<(VersionReason, String)> = a
            .list_versions(id)
            .unwrap()
            .into_iter()
            .map(|v| (v.reason, v.body))
            .collect();
        assert!(reasons.contains(&(VersionReason::MineBeforeMerge, MINE.into())));
        assert!(reasons.contains(&(VersionReason::TheirsBeforeMerge, THEIRS.into())));
        assert_eq!(a.conflicted_notes().unwrap(), vec![id]);
    }

    #[test]
    fn edits_of_different_paragraphs_merge_without_asking() {
        let (mut a, _) = device(1_000_000);
        let (mut b, _) = device(1_000_000);
        let id = note_with(&mut a, "Trip", BASE);
        sync(&mut a, &mut b);
        edit_body(&mut a, id, &BASE.replace("Monday", "Tuesday"));
        edit_body(
            &mut b,
            id,
            &BASE.replace("Pack light.", "Pack light, two bags at most."),
        );
        sync(&mut a, &mut b);
        assert!(a.note(id).unwrap().body.contains("Tuesday"));
        assert!(a.note(id).unwrap().body.contains("two bags"));
        assert_eq!(a.note_conflict(id).unwrap(), None);
        assert_eq!(b.note_conflict(id).unwrap(), None);
    }

    #[test]
    fn keeping_mine_reaches_the_other_device_and_settles_its_question() {
        let (mut a, mut b, id) = rewritten_on_both();
        a.resolve_conflict(id, &Resolution::Mine).unwrap();
        assert_eq!(a.note(id).unwrap().body, MINE);
        assert_eq!(a.note_conflict(id).unwrap(), None);
        let merged_kept = a
            .list_versions(id)
            .unwrap()
            .iter()
            .any(|v| v.body.contains("Alex") && v.body.contains("Friday"));
        assert!(merged_kept, "the merge stays in the history");
        sync(&mut a, &mut b);
        same(&a, &b);
        assert_eq!(b.note(id).unwrap().body, MINE);
        assert_eq!(b.note_conflict(id).unwrap(), None, "settled by a's choice");
    }

    #[test]
    fn keeping_theirs_and_keeping_the_merge() {
        let (mut a, mut b, id) = rewritten_on_both();
        a.resolve_conflict(id, &Resolution::Theirs).unwrap();
        assert_eq!(a.note(id).unwrap().body, THEIRS);
        b.resolve_conflict(id, &Resolution::Merged).unwrap();
        assert_eq!(b.note_conflict(id).unwrap(), None);
        sync(&mut a, &mut b);
        same(&a, &b);
        assert_eq!(b.note(id).unwrap().body, THEIRS);
        assert!(
            a.resolve_conflict(id, &Resolution::Mine).is_err(),
            "nothing left to decide"
        );
    }

    #[test]
    fn keeping_both_makes_a_second_note_beside_the_first() {
        let (mut a, mut b, id) = rewritten_on_both();
        a.set_note_tags(id, &["travel".into()]).unwrap();
        a.resolve_conflict(
            id,
            &Resolution::Both {
                copy_title: "Trip (other device)".into(),
            },
        )
        .unwrap();
        assert_eq!(a.note(id).unwrap().body, MINE);
        let copy = a
            .list_notes(NoteScope::All, NoteSort::TitleAsc)
            .unwrap()
            .into_iter()
            .find(|n| n.title == "Trip (other device)")
            .expect("the copy");
        let copy = a.note(copy.id).unwrap();
        assert_eq!(copy.body, THEIRS);
        assert_eq!(copy.tags, vec!["travel"]);
        sync(&mut a, &mut b);
        same(&a, &b);
    }

    #[test]
    fn a_note_deleted_here_and_edited_there_comes_back_marked() {
        let (mut a, _) = device(1_000_000);
        let (mut b, _) = device(1_000_000);
        let gone = note_with(&mut a, "Gone", "old");
        let trashed = note_with(&mut a, "Trashed", "old");
        sync(&mut a, &mut b);
        a.trash_note(gone).unwrap();
        a.purge_note(gone).unwrap();
        a.trash_note(trashed).unwrap();
        edit_body(&mut b, gone, "kept by b");
        edit_body(&mut b, trashed, "also kept");
        sync(&mut a, &mut b);
        for id in [gone, trashed] {
            let note = a.note(id).unwrap();
            assert!(note.trashed_at.is_some(), "in the trash with the edit");
            assert_eq!(
                a.note_conflict(id).unwrap().unwrap().kind,
                "editedAfterDelete"
            );
        }
        assert_eq!(a.note(gone).unwrap().body, "kept by b");
        a.restore_note(gone).unwrap();
        assert_eq!(a.note_conflict(gone).unwrap(), None, "restoring answers it");
        assert!(a.resolve_conflict(trashed, &Resolution::Mine).is_err());
        a.resolve_conflict(trashed, &Resolution::Merged).unwrap();
        assert_eq!(a.note_conflict(trashed).unwrap(), None);
    }

    #[test]
    fn a_save_from_an_older_screen_that_rewrote_the_same_paragraph_asks() {
        let (mut a, _) = device(1_000_000);
        let (mut b, _) = device(1_000_000);
        let id = note_with(&mut a, "Trip", BASE);
        sync(&mut a, &mut b);
        edit_body(&mut a, id, THEIRS);
        sync(&mut a, &mut b);
        // b's editor still had the base when it saved its rewrite
        let saved = b
            .update_note(
                id,
                NoteChange {
                    body: Some(MINE.into()),
                    base_body: Some(BASE.into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        assert!(saved.body.contains("Alex") && saved.body.contains("Friday"));
        let asked = b.note_conflict(id).unwrap().expect("b asks");
        assert_eq!(asked.mine.unwrap().body, MINE);
        assert_eq!(asked.theirs.unwrap().body, THEIRS);
    }
}

mod clocks {
    use super::*;
    use crate::clock::DAY;

    #[test]
    fn trash_from_a_device_whose_clock_is_far_behind_is_kept_for_the_full_time_here() {
        let (mut a, _) = device(1_000_000 + 400 * DAY);
        // b thinks it is a year ago
        let (mut b, b_clock) = device(1_000_000);
        let note = note_with(&mut a, "Kept", "x");
        let folder = a.create_folder(None, "Old").unwrap();
        sync(&mut a, &mut b);
        b_clock.advance(10 * DAY);
        b.trash_note(note).unwrap();
        b.trash_folder(folder.id).unwrap();
        sync(&mut a, &mut b);
        assert!(a.note(note).unwrap().trashed_at.unwrap() < 1_000_000 + 30 * DAY);
        // a starts again: dated a year ago, but it only arrived now
        assert_eq!(a.purge_expired_trash().unwrap(), 0);
        assert!(a.note(note).is_ok(), "still in the trash here");
        assert_eq!(a.list_trashed_folders_for_tests().len(), 1);
    }

    #[test]
    fn a_note_dated_ahead_by_a_fast_clock_still_gets_history_here() {
        let (mut a, _) = device(1_000_000 + 3 * DAY);
        let (mut b, b_clock) = device(1_000_000);
        let id = note_with(&mut a, "Plan", "from the future");
        sync(&mut a, &mut b);
        edit_body(&mut a, id, "edited in the future");
        sync(&mut a, &mut b);
        let before = b.list_versions(id).unwrap().len();
        edit_body(&mut b, id, "edited now");
        b_clock.advance(MINUTE);
        let after = b.list_versions(id).unwrap().len();
        assert!(
            after > before,
            "the text from the fast device is kept before this edit"
        );
        assert!(
            b.list_versions(id)
                .unwrap()
                .iter()
                .any(|v| v.body == "edited in the future")
        );
    }
}

mod files {
    use super::*;

    const A: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
    const B: &str = "60303ae22b998861bce3b28f33eec1be758a213c86c93c076dbe9f558c11c752";
    const THUMB: &str = "fd61a03af4f77d870fc21e05e7e80678095c92d808cfb3b5c279ee04c74aca13";

    fn names(s: &Store, id: NoteId) -> Vec<String> {
        s.list_attachments(id)
            .unwrap()
            .into_iter()
            .map(|a| a.file_name)
            .collect()
    }

    #[test]
    fn an_attachment_and_its_removal_reach_the_other_device() {
        let (mut a, _) = device(1_000_000);
        let (mut b, _) = device(1_000_000);
        let id = note_with(&mut a, "Garden", "![vines](attachments/vines.png)");
        let file = a
            .add_attachment(id, "vines.png", "image/png", 2048, A)
            .unwrap();
        sync(&mut a, &mut b);
        assert_eq!(b.list_attachments(id).unwrap(), vec![file.clone()]);
        b.set_attachment_thumb(file.id, THUMB).unwrap();
        sync(&mut a, &mut b);
        assert_eq!(
            a.list_attachments(id).unwrap()[0].thumb_sha256.as_deref(),
            Some(THUMB)
        );
        a.remove_attachment(file.id).unwrap();
        sync(&mut a, &mut b);
        assert!(b.list_attachments(id).unwrap().is_empty());
        same(&a, &b);
        // the file stays known on both until forgotten
        assert!(b.referenced_blobs().unwrap().contains(A));
    }

    #[test]
    fn the_same_name_attached_on_two_devices_at_once_keeps_both() {
        let (mut a, clock_a) = device(1_000_000);
        let (mut b, clock_b) = device(1_000_000);
        let id = note_with(&mut a, "Plans", "");
        sync(&mut a, &mut b);
        clock_a.advance(10);
        let first = a
            .add_attachment(id, "plan.pdf", "application/pdf", 1, A)
            .unwrap();
        clock_b.advance(20);
        b.add_attachment(id, "plan.pdf", "application/pdf", 2, B)
            .unwrap();
        sync(&mut a, &mut b);
        same(&a, &b);
        assert_eq!(names(&a, id), vec!["plan.pdf", "plan 2.pdf"]);
        // the earlier one keeps the name
        assert_eq!(a.list_attachments(id).unwrap()[0].id, first.id);
        // a local change to the note writes the settled names, nothing moves
        a.set_pinned(id, true).unwrap();
        sync(&mut a, &mut b);
        same(&a, &b);
        assert_eq!(names(&b, id), vec!["plan.pdf", "plan 2.pdf"]);
    }

    #[test]
    fn a_removal_and_a_thumbnail_on_two_devices_merge() {
        let (mut a, _) = device(1_000_000);
        let (mut b, _) = device(1_000_000);
        let id = note_with(&mut a, "Photos", "");
        let file = a.add_attachment(id, "p.jpg", "image/jpeg", 9, A).unwrap();
        sync(&mut a, &mut b);
        a.remove_attachment(file.id).unwrap();
        b.set_attachment_thumb(file.id, THUMB).unwrap();
        sync(&mut a, &mut b);
        same(&a, &b);
        assert!(a.list_attachments(id).unwrap().is_empty());
        // a new name in its place does not clash with the removed one
        b.add_attachment(id, "p.jpg", "image/jpeg", 9, B).unwrap();
        sync(&mut a, &mut b);
        same(&a, &b);
        assert_eq!(names(&a, id), vec!["p.jpg"]);
    }

    #[test]
    fn a_new_device_does_not_take_in_files_removed_before() {
        let (mut a, _) = device(1_000_000);
        let id = note_with(&mut a, "Old", "");
        let gone = a.add_attachment(id, "old.png", "image/png", 1, A).unwrap();
        a.add_attachment(id, "kept.png", "image/png", 1, B).unwrap();
        a.remove_attachment(gone.id).unwrap();
        // from snapshots, as a device linked later gets them
        let (mut b, _) = device(1_000_000);
        let space = a.sync_snapshot(crate::sync::SPACE).unwrap();
        b.apply_remote(crate::sync::SPACE, &space).unwrap();
        let snap = a.sync_snapshot(&id.to_string()).unwrap();
        b.apply_remote(&id.to_string(), &snap).unwrap();
        same(&a, &b);
        assert_eq!(b.referenced_blobs().unwrap().len(), 1);
    }

    #[test]
    fn attachments_of_a_vault_from_before_are_recorded_once() {
        let (mut a, _) = device(1_000_000);
        let id = note_with(&mut a, "Old note", "");
        a.add_attachment(id, "scan.pdf", "application/pdf", 77, A)
            .unwrap();
        a.wipe_sync_for_tests();
        a.bootstrap_sync_for_tests();
        let (mut b, _) = device(1_000_000);
        send(&mut a, &mut b);
        same(&a, &b);
        assert_eq!(names(&b, id), vec!["scan.pdf"]);
        let out = a.sync_outbox(usize::MAX).unwrap();
        assert!(out.is_empty());
        a.bootstrap_sync_for_tests();
        assert!(a.sync_outbox(1).unwrap().is_empty(), "recorded twice");
    }

    #[test]
    fn files_follow_a_note_trashed_restored_and_deleted() {
        let (mut a, _) = device(1_000_000);
        let (mut b, _) = device(1_000_000);
        let id = note_with(&mut a, "Receipt", "");
        a.add_attachment(id, "r.pdf", "application/pdf", 3, A)
            .unwrap();
        sync(&mut a, &mut b);
        a.trash_note(id).unwrap();
        sync(&mut a, &mut b);
        assert_eq!(names(&b, id), vec!["r.pdf"]);
        b.restore_note(id).unwrap();
        sync(&mut a, &mut b);
        a.trash_note(id).unwrap();
        a.purge_note(id).unwrap();
        sync(&mut a, &mut b);
        same(&a, &b);
        assert!(b.referenced_blobs().unwrap().is_empty());
    }
}
