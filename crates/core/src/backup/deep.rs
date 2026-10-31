// SPDX-License-Identifier: AGPL-3.0-or-later

//! Random backups: vaults of every shape round trip whole, sealed or not,
//! into an empty vault or one that kept part of them; the sealed reader
//! reads any position as the plain bytes; a damaged backup is refused or
//! restores exactly what it held, never something else. `DEEP_RUNS` sets
//! how many seeds (default 40).

use std::io::{Read, Seek, SeekFrom, Write};

use super::*;
use crate::store::test_support::store;
use crate::store::{NewNote, NoteChange};
use crate::testing::Rng;

const KDF: KdfParams = KdfParams::INSECURE_FOR_TESTS;

fn runs() -> u64 {
    std::env::var("DEEP_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40)
}

const TEXTS: &[&str] = &[
    "plain ",
    "été ",
    "[[Other]] ",
    "- [x] done\n",
    "```\ncode\n```\n",
    "😀 ",
    "\u{0}",
    "| a | b |\n",
    "",
];

fn text(rng: &mut Rng, words: usize) -> String {
    (0..words).map(|_| rng.pick(TEXTS).unwrap_or("")).collect()
}

/// Sizes around the 64 KB chunks, where sealing could go wrong.
fn size(rng: &mut Rng) -> usize {
    let near = [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK, 3 * CHUNK + 7];
    if rng.below(3) == 0 {
        rng.below(5000)
    } else {
        near[rng.below(near.len())]
    }
}

/// A random vault: a tree, notes with tags, pins, trash, history, files.
pub(crate) fn random_vault(rng: &mut Rng, dir: &Path) -> (Store, BlobStore) {
    let (mut s, clock) = store();
    let blobs = BlobStore::new(dir.join("blobs"), None);
    let mut folders = vec![None];
    for i in 0..rng.below(6) {
        let parent = folders[rng.below(folders.len())];
        let f = s
            .create_folder(parent, &format!("folder {i} {}", text(rng, 1)))
            .unwrap();
        folders.push(Some(f.id));
    }
    let mut notes = Vec::new();
    for i in 0..=rng.below(12) {
        let words = rng.below(8);
        let note = s
            .create_note(&NewNote {
                folder_id: folders[rng.below(folders.len())],
                title: if i == 0 { "Other".into() } else { text(rng, 2) },
                body: text(rng, words),
                tags: (0..rng.below(3))
                    .map(|_| rng.pick(&["home", "Work", "été"]).unwrap_or("x").to_owned())
                    .collect(),
                pinned: rng.below(4) == 0,
            })
            .unwrap();
        notes.push(note.id);
    }
    for _ in 0..rng.below(6) {
        clock.advance(crate::store::EDIT_SESSION + 1);
        let id = notes[rng.below(notes.len())];
        s.update_note(
            id,
            NoteChange {
                body: Some(text(rng, 3)),
                ..NoteChange::default()
            },
        )
        .unwrap();
    }
    for _ in 0..rng.below(5) {
        let id = notes[rng.below(notes.len())];
        let bytes: Vec<u8> = (0..size(rng))
            .map(|_| (rng.next_u64() & 0xff) as u8)
            .collect();
        let sha = blobs.put(&bytes).unwrap();
        let a = s
            .add_attachment(
                id,
                "f.bin",
                "application/octet-stream",
                i64::try_from(bytes.len()).unwrap(),
                &sha,
            )
            .unwrap();
        if rng.below(3) == 0 {
            let thumb = blobs.put(&bytes[..bytes.len().min(10)]).unwrap();
            s.set_attachment_thumb(a.id, &thumb).unwrap();
        }
    }
    for _ in 0..rng.below(3) {
        let id = notes[rng.below(notes.len())];
        let _ = s.trash_note(id);
    }
    (s, blobs)
}

pub(crate) fn assert_same(a: &BackupData, b: &BackupData, seed: u64) {
    assert_eq!(a.folders, b.folders, "seed {seed}: folders");
    assert_eq!(a.notes, b.notes, "seed {seed}: notes");
    assert_eq!(a.versions, b.versions, "seed {seed}: versions");
    assert_eq!(a.attachments, b.attachments, "seed {seed}: attachments");
}

#[test]
fn random_vaults_round_trip_whole() {
    for seed in 1..=runs() {
        let mut rng = Rng(seed * 0x9e37_79b9 + 1);
        let dir = tempfile::tempdir().unwrap();
        let (source, blobs) = random_vault(&mut rng, dir.path());
        let password = (rng.below(2) == 0).then_some("pass word");
        let path = dir.path().join("b.backup");
        write_backup(&source, &blobs, &path, password, KDF).unwrap();
        let original = source.backup_data().unwrap();

        // into an empty vault
        let (mut target, _) = store();
        let target_blobs = BlobStore::new(dir.path().join("t"), None);
        Backup::open(&path, password)
            .unwrap()
            .restore(&mut target, &target_blobs)
            .unwrap();
        assert_same(&original, &target.backup_data().unwrap(), seed);
        for sha in original.file_hashes() {
            assert_eq!(
                target_blobs.get(&sha).unwrap(),
                blobs.get(&sha).unwrap(),
                "seed {seed}: file"
            );
        }

        // into the same vault after losing part of it for good
        let mut source = source;
        for note in original.notes.iter().filter(|_| rng.below(2) == 0) {
            let _ = source.trash_note(note.id);
            source.purge_note(note.id).unwrap();
        }
        Backup::open(&path, password)
            .unwrap()
            .restore(&mut source, &blobs)
            .unwrap();
        let after = source.backup_data().unwrap();
        // notes trashed here before the loss come back as they were
        assert_eq!(after.notes.len(), original.notes.len(), "seed {seed}");
        for note in &original.notes {
            let back = after.notes.iter().find(|n| n.id == note.id).unwrap();
            assert_eq!(
                (
                    &back.title,
                    &back.body,
                    &back.tags,
                    back.pinned,
                    back.created_at
                ),
                (
                    &note.title,
                    &note.body,
                    &note.tags,
                    note.pinned,
                    note.created_at
                ),
                "seed {seed}"
            );
        }
    }
}

#[test]
fn the_sealed_reader_reads_any_position_as_the_plain_bytes() {
    let key = app_e2ee::files_key_from(&[7u8; 32]);
    for seed in 1..=runs() {
        let mut rng = Rng(seed * 31 + 5);
        let len = size(&mut rng) + rng.below(3) * CHUNK;
        let plain: Vec<u8> = (0..len).map(|_| (rng.next_u64() & 0xff) as u8).collect();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sealed");
        {
            let mut file = File::create(&path).unwrap();
            file.write_all(b"head").unwrap();
            let mut writer = SealingWriter::new(file, &key).unwrap();
            // written in pieces of every size
            let mut at = 0;
            while at < plain.len() {
                let n = (1 + rng.below(CHUNK + 100)).min(plain.len() - at);
                writer.write_all(&plain[at..at + n]).unwrap();
                at += n;
            }
            writer.finish().unwrap();
        }
        let mut reader = SealedReader::new(File::open(&path).unwrap(), 4, &key).unwrap();
        assert_eq!(reader.size, len as u64, "seed {seed}");
        for _ in 0..30 {
            let from = rng.below(len + 1);
            let want = rng.below(CHUNK * 2);
            reader.seek(SeekFrom::Start(from as u64)).unwrap();
            let mut out = Vec::new();
            (&mut reader)
                .take(want as u64)
                .read_to_end(&mut out)
                .unwrap();
            let end = (from + want).min(len);
            assert_eq!(out, plain[from..end], "seed {seed} at {from}");
        }
        let mut all = Vec::new();
        reader.seek(SeekFrom::Start(0)).unwrap();
        reader.read_to_end(&mut all).unwrap();
        assert_eq!(all, plain, "seed {seed}");
    }
}

#[test]
fn a_damaged_backup_is_refused_or_restores_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let mut rng = Rng(0x5eed);
    let (source, blobs) = random_vault(&mut rng, dir.path());
    let original = source.backup_data().unwrap();
    for (sealed, name) in [(false, "plain"), (true, "sealed")] {
        let password = sealed.then_some("pass word");
        let path = dir.path().join(name);
        write_backup(&source, &blobs, &path, password, KDF).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let (mut refused, mut whole) = (0, 0);
        for seed in 1..=runs() * 3 {
            let mut damaged = bytes.clone();
            if seed % 3 == 0 {
                damaged.truncate(rng.below(bytes.len()));
            } else {
                let at = rng.below(bytes.len());
                damaged[at] ^= 1 + u8::try_from(rng.below(255)).unwrap();
            }
            let bad = dir.path().join(format!("{name}-{seed}"));
            std::fs::write(&bad, &damaged).unwrap();
            let (mut target, _) = store();
            let target_blobs = BlobStore::new(dir.path().join(format!("t{name}{seed}")), None);
            let result = Backup::open(&bad, password)
                .and_then(|mut b| b.restore(&mut target, &target_blobs));
            if result.is_ok() {
                whole += 1;
                assert_same(&original, &target.backup_data().unwrap(), seed);
            } else {
                refused += 1;
                // nothing half written
                assert!(
                    target.backup_data().unwrap().notes.is_empty(),
                    "{name} {seed}"
                );
            }
            let _ = std::fs::remove_file(&bad);
        }
        // sealed: every change is caught; plain: a change in parts nobody
        // reads (zip padding, timestamps) can pass
        if sealed {
            assert_eq!(whole, 0, "a damaged sealed backup restored");
        }
        assert!(refused > 0);
    }
}

#[test]
fn a_hostile_header_cannot_ask_for_all_memory() {
    let dir = tempfile::tempdir().unwrap();
    let (source, blobs) = random_vault(&mut Rng(3), dir.path());
    let path = dir.path().join("h");
    write_backup(&source, &blobs, &path, Some("pw"), KDF).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    // memory: 4 TiB in KiB
    bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        Backup::open(&path, Some("pw")),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn backups_open_from_a_file_handle() {
    let dir = tempfile::tempdir().unwrap();
    let (source, blobs) = random_vault(&mut Rng(9), dir.path());
    let path = dir.path().join("handle");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .unwrap();
    write_backup_to(&source, &blobs, file, Some("pw"), KDF).unwrap();
    let mut file = File::open(&path).unwrap();
    assert!(is_encrypted_file(&mut file).unwrap());
    let (mut target, _) = store();
    Backup::open_file(file, Some("pw"))
        .unwrap()
        .restore(&mut target, &BlobStore::new(dir.path().join("t"), None))
        .unwrap();
    assert_same(
        &source.backup_data().unwrap(),
        &target.backup_data().unwrap(),
        9,
    );
}
