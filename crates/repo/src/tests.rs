// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use super::target::MemoryTarget;
use super::*;

const KDF: KdfParams = KdfParams::INSECURE_FOR_TESTS;

fn memory() -> (Arc<MemoryTarget>, Repo) {
    let target = Arc::new(MemoryTarget::default());
    let repo = Repo::init(target.clone(), "pw", KDF, 0).unwrap();
    (target, repo)
}

/// A snapshot of named contents, as a caller makes one.
fn snapshot(repo: &Repo, time: i64, contents: &[(&str, Vec<u8>)]) -> String {
    let index = repo.index().unwrap();
    let mut writer = Writer::new(repo, &index);
    let mut items = Vec::new();
    for (name, bytes) in contents {
        let (objects, size) = writer.add_stream(&mut bytes.as_slice()).unwrap();
        items.push(Item {
            name: (*name).to_owned(),
            size,
            objects,
        });
    }
    writer.finish().unwrap();
    let mut snapshot = Snapshot {
        id: String::new(),
        time,
        host: "test".into(),
        kind: "test".into(),
        app_version: "0".into(),
        items,
        counts: BTreeMap::new(),
    };
    repo.save_snapshot(&mut snapshot).unwrap();
    snapshot.id
}

fn read(repo: &Repo, id: &str, name: &str) -> Vec<u8> {
    let snapshot = repo.snapshot(id).unwrap();
    let mut out = Vec::new();
    repo.read_item(
        &repo.index().unwrap(),
        snapshot.item(name).unwrap(),
        &mut out,
    )
    .unwrap();
    out
}

fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed * 2 + 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            u8::try_from(x & 0xff).unwrap()
        })
        .collect()
}

#[test]
fn a_repository_opens_with_its_password_only() {
    let dir = tempfile::tempdir().unwrap();
    let target = Arc::new(FolderTarget::new(dir.path().join("repo")));
    Repo::init(target.clone(), "backup password", KDF, 0).unwrap();
    assert!(matches!(
        Repo::init(target.clone(), "other", KDF, 0),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        Repo::open(target.clone(), "wrong"),
        Err(Error::WrongPassword)
    ));
    let repo = Repo::open(target.clone(), "backup password").unwrap();
    repo.change_password("new password", KDF, 1).unwrap();
    assert!(matches!(
        Repo::open(target.clone(), "backup password"),
        Err(Error::WrongPassword)
    ));
    Repo::open(target.clone(), "new password").unwrap();
    let empty = Arc::new(FolderTarget::new(dir.path().join("none")));
    assert!(matches!(Repo::open(empty, "x"), Err(Error::NotFound(_))));
    assert!(matches!(
        Repo::init(Arc::new(MemoryTarget::default()), "", KDF, 0),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn contents_come_back_and_are_stored_once() {
    let (target, repo) = memory();
    let big = noise(1, 9 * 1024 * 1024);
    let small = b"a small thing".to_vec();
    let first = snapshot(
        &repo,
        1,
        &[
            ("big", big.clone()),
            ("small", small.clone()),
            ("empty", Vec::new()),
        ],
    );
    assert_eq!(read(&repo, &first, "big"), big);
    assert_eq!(read(&repo, &first, "small"), small);
    assert!(read(&repo, &first, "empty").is_empty());
    let stored = |t: &MemoryTarget| -> usize {
        t.files()
            .iter()
            .filter(|(p, _)| p.starts_with("data/"))
            .map(|(_, b)| b.len())
            .sum()
    };
    let after_first = stored(&target);
    // the same again, and the big one with a change in the middle
    let mut changed = big.clone();
    changed.splice(4_000_000..4_000_000, *b"inserted");
    let second = snapshot(&repo, 2, &[("big", changed.clone()), ("copy", big.clone())]);
    assert_eq!(read(&repo, &second, "big"), changed);
    assert_eq!(read(&repo, &second, "copy"), big);
    let grew = stored(&target) - after_first;
    assert!(grew < 3 * 1024 * 1024, "{grew} bytes for a small change");
    // compressible content takes less room than it is
    let text = b"the same words again and again ".repeat(100_000);
    let before = stored(&target);
    snapshot(&repo, 3, &[("text", text.clone())]);
    assert!(stored(&target) - before < text.len() / 10);
}

#[test]
fn locks_keep_a_prune_alone() {
    let (_, repo) = memory();
    let shared = repo.lock("laptop", false, 1000).unwrap();
    let other = repo.lock("phone", false, 1000).unwrap();
    assert!(matches!(
        repo.lock("laptop", true, 2000),
        Err(Error::Invalid(_))
    ));
    shared.release().unwrap();
    other.release().unwrap();
    let exclusive = repo.lock("laptop", true, 3000).unwrap();
    assert!(repo.lock("phone", false, 3000).is_err());
    // a lock from a run that died stops counting
    assert!(repo.lock("phone", false, 3000 + maintain::STALE_MS).is_ok());
    drop(exclusive);
}

#[test]
fn prune_keeps_every_used_object_and_frees_the_rest() {
    let (target, repo) = memory();
    let mut ids = Vec::new();
    for day in 0..12 {
        ids.push(snapshot(
            &repo,
            day,
            &[
                ("kept", noise(99, 200_000)),
                ("daily", noise(day.cast_unsigned() + 1, 700_000)),
            ],
        ));
    }
    let gone: HashSet<String> = ids[..10].iter().cloned().collect();
    repo.forget(&gone).unwrap();
    let report = repo.prune("test", 20).unwrap();
    assert!(report.objects_removed >= 10, "{report:?}");
    for id in &ids[10..] {
        assert_eq!(read(&repo, id, "kept"), noise(99, 200_000));
    }
    let check = repo.check(true).unwrap();
    assert!(check.problems.is_empty(), "{check:?}");
    assert!(
        target.files().keys().all(|p| !p.starts_with("locks/")),
        "the lock is gone"
    );
    // a second prune has nothing to do
    let again = repo.prune("test", 21).unwrap();
    assert_eq!(again.objects_removed, 0);
    assert_eq!(again.packs_rewritten, 0);
}

#[test]
fn a_file_of_one_kind_cannot_pass_for_another() {
    let (target, repo) = memory();
    let id = snapshot(&repo, 1, &[("x", b"hello".to_vec())]);
    let index_path = target
        .files()
        .keys()
        .find(|p| p.starts_with("index/"))
        .unwrap()
        .clone();
    let index_bytes = target.files()[&index_path].clone();
    target
        .files()
        .insert(format!("snapshots/{id}"), index_bytes);
    assert!(matches!(repo.snapshot(&id), Err(Error::Corrupt(_))));
    // another repository's files do not open either
    let (_, other) = memory();
    assert!(matches!(
        other.read_json::<serde_json::Value>(b"index", &index_path),
        Err(Error::NotFound(_) | Error::Corrupt(_))
    ));
}
