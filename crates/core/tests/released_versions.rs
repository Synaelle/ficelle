// SPDX-License-Identifier: AGPL-3.0-or-later

//! Vaults and backups made by released versions of the app, opened and
//! restored by today's code: old vaults must keep opening and old backups
//! must stay restorable.
//!
//! Each folder of `tests/fixtures` is named after the version that made it
//! and whether its vault is encrypted. The released Linux app filled the
//! vault through its own commands, then saved a backup of it with its
//! Backup command: four notes and one in the trash, tags (one nested and,
//! from 0.6, coloured), a pinned note with an attachment, a note with
//! history, a title edited from two screens at once (a conflict), a note in
//! a folder up to 0.8.6 (folders became tags), and from 0.6 a synced
//! setting. The files are the ones the app wrote and are never edited: a
//! test opens a copy.

use std::fs;
use std::path::Path;

use app_core::backup::Backup;
use app_core::blobs::{BlobStore, sha256_hex};
use app_core::model::{NoteScope, NoteSort};
use app_core::store::Store;
use app_core::vault::{Encryption, Vault};

/// The encrypted vaults' password.
const VAULT_PASSWORD: &str = "fixture vault password";
/// The encrypted vaults' backups' password.
const BACKUP_PASSWORD: &str = "fixture backup password";

/// Versions with a fixture.
const VERSIONS: &[&str] = &["0.5.5", "0.6.0", "0.7.1", "0.8.6", "0.8.7"];

/// The attachment, a 2×2 png.
const PICTURE_SHA256: &str = "ddf3f5b3e700b520f43b2f6f8e511eea2006b97410a231b28e7a01c141974f0a";

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Where a fixture comes from, said in every failure.
struct Made {
    version: &'static str,
    encrypted: bool,
    /// Read from its backup rather than from its vault.
    restored: bool,
}

impl std::fmt::Display for Made {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.version,
            if self.encrypted { "encrypted" } else { "plain" },
            if self.restored { "backup" } else { "vault" }
        )
    }
}

/// Checks a store holds everything the released app wrote.
fn holds_everything(store: &Store, blobs: &BlobStore, made: &Made) {
    let since_0_6 = made.version != "0.5.5";
    let live = store
        .list_notes(NoteScope::All, NoteSort::TitleAsc)
        .unwrap();
    let titles: Vec<&str> = live.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Garden plan",
            "Paint the fence red green",
            "Seed list",
            "Watering"
        ],
        "{made}"
    );
    let note = |title: &str| {
        store
            .note(live.iter().find(|n| n.title == title).unwrap().id)
            .unwrap()
    };

    let plan = note("Garden plan");
    assert!(plan.pinned, "{made}");
    // a 0.5.5 backup's folder Garden comes back as a tag and may give the
    // tag its spelling
    let lower: Vec<String> = plan.tags.iter().map(|t| t.to_lowercase()).collect();
    assert_eq!(lower, ["garden", "garden/beds"], "{made}");
    assert!(plan.body.contains("- [x] order seeds"), "{made}");
    let files = store.list_attachments(plan.id).unwrap();
    assert_eq!(files.len(), 1, "{made}");
    assert_eq!(
        (
            files[0].file_name.as_str(),
            files[0].size,
            files[0].sha256.as_str()
        ),
        ("bed.png", 79, PICTURE_SHA256),
        "{made}"
    );
    let picture = blobs
        .get(PICTURE_SHA256)
        .unwrap()
        .unwrap_or_else(|| panic!("{made}"));
    assert_eq!(sha256_hex(&picture), PICTURE_SHA256, "{made}");

    // up to 0.8.6 it sat in a folder, which is a tag of the same name now
    let seeds = note("Seed list");
    assert!(seeds.body.starts_with("| Seed | When |"), "{made}");
    assert!(
        seeds.tags.iter().any(|t| t.eq_ignore_ascii_case("garden")),
        "{made}: {:?}",
        seeds.tags
    );

    let watering = note("Watering");
    assert_eq!(
        watering.body, "Water in the morning when it is hot.",
        "{made}"
    );
    let versions = store.list_versions(watering.id).unwrap();
    assert!(
        versions.iter().any(|v| v.body == "Water every evening."),
        "{made}"
    );

    let fence = note("Paint the fence red green");
    assert_eq!(fence.body, "Before the summer.", "{made}");

    let trash = store
        .list_notes(NoteScope::Trash, NoteSort::TitleAsc)
        .unwrap();
    assert_eq!(trash.len(), 1, "{made}");
    let old = store.note(trash[0].id).unwrap();
    assert_eq!(
        (old.title.as_str(), old.body.as_str()),
        ("Old ideas", "A pond by the fence."),
        "{made}"
    );
    assert!(old.trashed_at.is_some(), "{made}");

    let tags = store.list_tags().unwrap();
    let beds = tags
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case("garden/beds"))
        .unwrap_or_else(|| panic!("{made}"));
    assert_eq!(
        beds.color.as_deref(),
        since_0_6.then_some("green"),
        "{made}"
    );
}

/// Checks a vault kept what stays with it and a backup leaves out: the
/// conflict waiting to be settled here, the synced settings.
fn holds_its_own_state(store: &Store, made: &Made) {
    let since_0_6 = made.version != "0.5.5";
    let fence = store
        .list_notes(NoteScope::All, NoteSort::TitleAsc)
        .unwrap()
        .into_iter()
        .find(|n| n.title == "Paint the fence red green")
        .unwrap();
    let conflict = store
        .note_conflict(fence.id)
        .unwrap()
        .unwrap_or_else(|| panic!("{made}"));
    assert_eq!(conflict.kind, "merge", "{made}");
    assert_eq!(
        conflict.mine.map(|s| s.title).as_deref(),
        Some("Paint the fence red"),
        "{made}"
    );
    assert_eq!(
        conflict.theirs.map(|s| s.title).as_deref(),
        Some("Paint the fence green"),
        "{made}"
    );
    let theme = store
        .synced_settings()
        .unwrap()
        .into_iter()
        .find(|(key, _)| key == "theme")
        .map(|(_, value)| value);
    assert_eq!(theme.as_deref(), since_0_6.then_some("\"dark\""), "{made}");
}

#[test]
fn vaults_made_by_released_versions_open_with_everything_in_them() {
    for version in VERSIONS {
        for encrypted in [false, true] {
            let made = Made {
                version,
                encrypted,
                restored: false,
            };
            let name = format!(
                "{version}-{}",
                if encrypted { "encrypted" } else { "plain" }
            );
            let dir = tempfile::tempdir().unwrap();
            copy_dir(&fixture(&name).join("vault"), dir.path());
            let vault = Vault::open(dir.path()).unwrap();
            assert_eq!(vault.is_encrypted(), encrypted, "{made}");
            let mut store = if encrypted {
                vault.unlock_with_password(VAULT_PASSWORD)
            } else {
                vault.unlock_plain()
            }
            .unwrap_or_else(|e| panic!("{made}: {e}"));
            let blobs = vault.blobs(&mut store).unwrap();
            holds_everything(&store, &blobs, &made);
            holds_its_own_state(&store, &made);
        }
    }
}

#[test]
fn backups_made_by_released_versions_restore_into_a_new_vault() {
    for version in VERSIONS {
        for encrypted in [false, true] {
            let made = Made {
                version,
                encrypted,
                restored: true,
            };
            let name = format!(
                "{version}-{}",
                if encrypted { "encrypted" } else { "plain" }
            );
            let mut backup = Backup::open(
                &fixture(&name).join("backup.ficelle"),
                encrypted.then_some(BACKUP_PASSWORD),
            )
            .unwrap_or_else(|e| panic!("{made}: {e}"));
            assert_eq!(backup.manifest().app_version, *version, "{made}");
            let dir = tempfile::tempdir().unwrap();
            let (vault, mut store) = Vault::create(dir.path(), Encryption::Off).unwrap();
            let blobs = vault.blobs(&mut store).unwrap();
            let report = backup
                .restore(&mut store, &blobs)
                .unwrap_or_else(|e| panic!("{made}: {e}"));
            assert_eq!(
                (
                    report.notes,
                    report.files,
                    report.files_missing,
                    report.skipped
                ),
                (5, 1, 0, 0),
                "{made}"
            );
            holds_everything(&store, &blobs, &made);
        }
    }
}

/// 0.8.7 wrote its sync documents before they carried a format number, and
/// its vault needs no migration to open: what another device on 0.8.7
/// sends.
#[test]
fn sync_documents_from_before_the_format_number_merge() {
    let old = tempfile::tempdir().unwrap();
    copy_dir(&fixture("0.8.7-plain").join("vault"), old.path());
    let mut sender = Vault::open(old.path()).unwrap().unlock_plain().unwrap();
    let changes = sender.peer_changes(&[]).unwrap();

    let new = tempfile::tempdir().unwrap();
    let (_, mut store) = Vault::create(new.path(), Encryption::Off).unwrap();
    assert!(store.peer_take(&changes).unwrap() > 0);
    let titles = |s: &Store, scope| -> Vec<String> {
        s.list_notes(scope, NoteSort::TitleAsc)
            .unwrap()
            .into_iter()
            .map(|n| n.title)
            .collect()
    };
    assert_eq!(
        titles(&store, NoteScope::All),
        titles(&sender, NoteScope::All)
    );
    assert_eq!(titles(&store, NoteScope::Trash), ["Old ideas"]);
}
