// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashSet;
use std::sync::Arc;

use app_repo::target::MemoryTarget;

use super::*;
use crate::backup::deep::{assert_same, random_vault};
use crate::blobs::BlobStore;
use crate::error::Error;
use crate::store::test_support::store;
use crate::store::{NewNote, NoteChange};
use crate::testing::Rng;

const KDF: KdfParams = KdfParams::INSECURE_FOR_TESTS;
const DAY: i64 = 86_400_000;

fn folder_repo(dir: &std::path::Path) -> (Arc<FolderTarget>, Repo) {
    let target = Arc::new(FolderTarget::new(dir.join("repo")));
    let repo = Repo::init(target.clone(), "backup password", KDF, 0).unwrap();
    (target, repo)
}

#[test]
fn a_vault_comes_back_whole_and_nothing_is_readable_at_the_target() {
    for seed in 1..=12 {
        let mut rng = Rng(seed * 0x9e37_79b9 + 7);
        let dir = tempfile::tempdir().unwrap();
        let (source, blobs) = random_vault(&mut rng, dir.path());
        let (_, repo) = folder_repo(dir.path());
        let run = backup_vault(&repo, &source, &blobs, "laptop", &|| 1000).unwrap();
        assert!(!run.snapshot.is_empty());

        let (mut target, _) = store();
        let target_blobs = BlobStore::new(dir.path().join("restored"), None);
        restore_vault(&repo, &run.snapshot, &mut target, &target_blobs).unwrap();
        assert_same(
            &source.backup_data().unwrap(),
            &target.backup_data().unwrap(),
            seed,
        );
        for sha in source.backup_data().unwrap().file_hashes() {
            assert!(target_blobs.has(&sha).unwrap(), "seed {seed}: file {sha}");
        }
        // twice changes nothing
        let again = restore_vault(&repo, &run.snapshot, &mut target, &target_blobs).unwrap();
        assert_eq!(again.notes, 0);

        // no note text or title appears in what the target holds
        let titles: Vec<String> = source
            .backup_data()
            .unwrap()
            .notes
            .iter()
            .map(|n| n.title.clone())
            .filter(|t| t.len() > 6)
            .collect();
        for file in repo.target().list("").unwrap() {
            let bytes = repo.target().get(&file.path).unwrap();
            for title in &titles {
                assert!(
                    !bytes.windows(title.len()).any(|w| w == title.as_bytes()),
                    "seed {seed}: {title} readable in {}",
                    file.path
                );
            }
        }
    }
}

#[test]
fn runs_store_only_what_changed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _) = store();
    let blobs = BlobStore::new(dir.path().join("blobs"), None);
    let mut notes = Vec::new();
    for i in 0..200 {
        notes.push(
            s.create_note(&NewNote {
                title: format!("Note {i}"),
                body: format!("body of note {i} ").repeat(40),
                ..NewNote::default()
            })
            .unwrap()
            .id,
        );
    }
    let mut rng = Rng(5);
    let big: Vec<u8> = (0..6 * 1024 * 1024)
        .map(|_| u8::try_from(rng.next_u64() & 0xff).unwrap())
        .collect();
    let sha = blobs.put(&big).unwrap();
    s.add_attachment(
        notes[0],
        "big.bin",
        "application/octet-stream",
        6 * 1024 * 1024,
        &sha,
    )
    .unwrap();
    let (_, repo) = folder_repo(dir.path());

    let first = backup_vault(&repo, &s, &blobs, "laptop", &|| DAY).unwrap();
    assert!(first.stats.new_objects >= 205, "{:?}", first.stats);
    assert!(first.stats.packs >= 1);

    let same = backup_vault(&repo, &s, &blobs, "laptop", &|| 2 * DAY).unwrap();
    assert_eq!(same.stats.new_objects, 0, "{:?}", same.stats);
    // the big file was taken from the last snapshot, not read again
    assert!(same.stats.read_bytes < 1024 * 1024, "{:?}", same.stats);

    s.update_note(
        notes[7],
        NoteChange {
            body: Some("changed".into()),
            ..NoteChange::default()
        },
    )
    .unwrap();
    let one = backup_vault(&repo, &s, &blobs, "laptop", &|| 3 * DAY).unwrap();
    assert!(
        (1..=2).contains(&one.stats.new_objects),
        "one note changed: {:?}",
        one.stats
    );

    // another device with the same file stores nothing of it again
    let (mut other, _) = store();
    let other_blobs = BlobStore::new(dir.path().join("other"), None);
    let n = other.create_note(&NewNote::default()).unwrap();
    assert_eq!(other_blobs.put(&big).unwrap(), sha);
    other
        .add_attachment(
            n.id,
            "same.bin",
            "application/octet-stream",
            6 * 1024 * 1024,
            &sha,
        )
        .unwrap();
    let phone = backup_vault(&repo, &other, &other_blobs, "phone", &|| 4 * DAY).unwrap();
    assert!(phone.stats.new_bytes < 64 * 1024, "{:?}", phone.stats);
    assert_eq!(repo.snapshots().unwrap().len(), 4);
}

#[test]
fn forgetting_and_pruning_frees_space_and_keeps_what_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _) = store();
    let blobs = BlobStore::new(dir.path().join("blobs"), None);
    let (_, repo) = folder_repo(dir.path());
    let note = s.create_note(&NewNote::default()).unwrap();
    // a new large file every day for 40 days
    for day in 0..40_i64 {
        let mut rng = Rng(u64::try_from(day).unwrap() + 11);
        let bytes: Vec<u8> = (0..300 * 1024)
            .map(|_| u8::try_from(rng.next_u64() & 0xff).unwrap())
            .collect();
        let sha = blobs.put(&bytes).unwrap();
        let a = s
            .add_attachment(
                note.id,
                "day.bin",
                "application/octet-stream",
                300 * 1024,
                &sha,
            )
            .unwrap();
        backup_vault(&repo, &s, &blobs, "laptop", &|| day * DAY + 1).unwrap();
        // gone the next day: only the snapshots hold it
        s.remove_attachment(a.id).unwrap();
    }
    let all: Vec<(String, i64)> = repo
        .snapshots()
        .unwrap()
        .iter()
        .map(|s| (s.id.clone(), s.time))
        .collect();
    let policy = Policy {
        last: 0,
        daily: 3,
        weekly: 0,
        monthly: 0,
        ..Policy::default()
    };
    let kept = keep(&all, policy, 0);
    assert_eq!(kept.len(), 3);
    let before: u64 = repo
        .target()
        .list("data/")
        .unwrap()
        .iter()
        .map(|l| l.size)
        .sum();
    let gone: HashSet<String> = all
        .iter()
        .map(|(id, _)| id.clone())
        .filter(|id| !kept.contains(id))
        .collect();
    repo.forget(&gone).unwrap();
    let report = repo.prune("laptop", 50 * DAY).unwrap();
    let after: u64 = repo
        .target()
        .list("data/")
        .unwrap()
        .iter()
        .map(|l| l.size)
        .sum();
    assert!(after < before / 5, "{before} to {after}, {report:?}");
    assert!(report.objects_removed >= 30, "{report:?}");
    let check = repo.check(true).unwrap();
    assert!(check.problems.is_empty(), "{check:?}");
    assert_eq!(check.snapshots, 3);
    // what is kept still restores
    for id in &kept {
        let t = test_restore(&repo, id, &dir.path().join(format!("scratch-{id}"))).unwrap();
        assert!(t.problems.is_empty(), "{t:?}");
        assert_eq!(t.restored.files, 1);
    }
}

#[test]
fn a_run_cut_short_leaves_the_repository_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _) = store();
    let blobs = BlobStore::new(dir.path().join("blobs"), None);
    for i in 0..50 {
        s.create_note(&NewNote {
            title: format!("n{i}"),
            body: "x".repeat(i * 1000),
            ..NewNote::default()
        })
        .unwrap();
    }
    let target = Arc::new(MemoryTarget::default());
    let repo = Repo::init(target.clone(), "pw", KDF, 0).unwrap();
    let first = backup_vault(&repo, &s, &blobs, "laptop", &|| DAY).unwrap();
    for n in 0..60 {
        s.create_note(&NewNote {
            title: format!("more {n}"),
            body: format!("{n}").repeat(20_000),
            ..NewNote::default()
        })
        .unwrap();
        *target.fail_after.lock().unwrap() = Some(n);
        let cut = backup_vault(&repo, &s, &blobs, "laptop", &|| 2 * DAY);
        *target.fail_after.lock().unwrap() = None;
        if cut.is_ok() {
            continue;
        }
        let check = repo.check(true).unwrap();
        assert!(check.problems.is_empty(), "cut after {n} calls: {check:?}");
        let (mut back, _) = store();
        restore_vault(
            &repo,
            &first.snapshot,
            &mut back,
            &BlobStore::new(dir.path().join(format!("r{n}")), None),
        )
        .unwrap();
        assert_eq!(back.backup_data().unwrap().notes.len(), 50);
    }
    // the cut runs' leftovers go at the next prune, after a full run
    let last = backup_vault(&repo, &s, &blobs, "laptop", &|| 3 * DAY).unwrap();
    repo.prune("laptop", 3 * DAY).unwrap();
    let check = repo.check(true).unwrap();
    assert!(check.problems.is_empty(), "{check:?}");
    let indexed: HashSet<String> = repo.index().unwrap().packs.into_keys().collect();
    for file in repo.target().list("data/").unwrap() {
        assert!(
            indexed.contains(file.path.rsplit('/').next().unwrap()),
            "{} left over",
            file.path
        );
    }
    let (mut back, _) = store();
    restore_vault(
        &repo,
        &last.snapshot,
        &mut back,
        &BlobStore::new(dir.path().join("last"), None),
    )
    .unwrap();
    assert_eq!(back.backup_data().unwrap().notes.len(), 110);
}

#[test]
fn damage_is_found_and_never_restored() {
    let dir = tempfile::tempdir().unwrap();
    let mut rng = Rng(99);
    let (source, blobs) = random_vault(&mut rng, dir.path());
    let target = Arc::new(MemoryTarget::default());
    let repo = Repo::init(target.clone(), "pw", KDF, 0).unwrap();
    let run = backup_vault(&repo, &source, &blobs, "laptop", &|| 1).unwrap();
    assert!(repo.check(true).unwrap().problems.is_empty());
    {
        let mut files = target.files();
        let pack = files
            .keys()
            .find(|k| k.starts_with("data/"))
            .unwrap()
            .clone();
        let bytes = files.get_mut(&pack).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0x40;
    }
    let check = repo.check(true).unwrap();
    assert!(
        check.problems.iter().any(|p| p.contains("damaged")),
        "{check:?}"
    );
    // a quick check does not read packs: it finds nothing
    assert!(repo.check(false).unwrap().problems.is_empty());
    let (mut back, _) = store();
    let restored = restore_vault(
        &repo,
        &run.snapshot,
        &mut back,
        &BlobStore::new(dir.path().join("b"), None),
    );
    assert!(
        matches!(restored, Err(Error::Corrupt(_) | Error::Io(_))),
        "{restored:?}"
    );
    let t = test_restore(&repo, &run.snapshot, &dir.path().join("scratch")).unwrap();
    assert!(!t.problems.is_empty());
    assert!(
        !dir.path().join("scratch").join("notes.db").exists(),
        "the scratch vault is removed"
    );
}

/// Costs at scale, run by hand in release:
/// `cargo test -p app-core --release backup_costs -- --ignored --nocapture`
#[test]
#[ignore = "a measurement, run by hand"]
fn backup_costs() {
    use std::time::Instant;
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _) = store();
    let blobs = BlobStore::new(dir.path().join("blobs"), None);
    let mut rng = Rng(42);
    let mut notes = Vec::new();
    for i in 0..3000 {
        notes.push(
            s.create_note(&NewNote {
                title: format!("Note {i}"),
                body: format!("words of note {i} ").repeat(60),
                ..NewNote::default()
            })
            .unwrap()
            .id,
        );
    }
    for i in 0..300 {
        let bytes: Vec<u8> = (0..100_000)
            .map(|_| (rng.next_u64() & 0xff) as u8)
            .collect();
        let sha = blobs.put(&bytes).unwrap();
        s.add_attachment(
            notes[i * 10],
            "f.bin",
            "application/octet-stream",
            100_000,
            &sha,
        )
        .unwrap();
    }
    let (target, repo) = folder_repo(dir.path());
    let time = |what: &str, f: &mut dyn FnMut()| {
        let started = Instant::now();
        f();
        println!("{what}: {:.2} s", started.elapsed().as_secs_f64());
    };
    let day = std::cell::Cell::new(0);
    let run = |label: &str, s: &crate::store::Store| {
        day.set(day.get() + 1);
        let started = Instant::now();
        let r = backup_vault(&repo, s, &blobs, "laptop", &|| day.get() * DAY).unwrap();
        println!(
            "{label}: {:.2} s, {} new objects, {} KB stored, {} packs",
            started.elapsed().as_secs_f64(),
            r.stats.new_objects,
            r.stats.stored_bytes / 1024,
            r.stats.packs
        );
        r.snapshot
    };
    let first = run("first backup", &s);
    run("nothing changed", &s);
    for id in &notes[..30] {
        s.update_note(
            *id,
            NoteChange {
                body: Some("changed".into()),
                ..NoteChange::default()
            },
        )
        .unwrap();
    }
    run("30 notes changed", &s);
    let size: u64 = target.list("").unwrap().iter().map(|l| l.size).sum();
    println!("repository: {} MB", size / 1024 / 1024);
    time("full check", &mut || {
        assert!(repo.check(true).unwrap().problems.is_empty());
    });
    time("restore into an empty vault", &mut || {
        let (mut back, _) = store();
        let b = BlobStore::new(dir.path().join("back"), None);
        let r = restore_vault(&repo, &first, &mut back, &b).unwrap();
        assert_eq!(r.notes, 3000);
    });
    time("test restore", &mut || {
        let t = test_restore(&repo, &first, &dir.path().join("scratch")).unwrap();
        assert!(t.problems.is_empty(), "{t:?}");
    });
}
