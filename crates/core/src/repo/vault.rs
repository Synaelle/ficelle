// SPDX-License-Identifier: AGPL-3.0-or-later

//! An app's notes in a repository. A snapshot holds what the backup
//! archive holds (folders, notes with ids, tags, history, file
//! descriptions, files), one item per note so a note that did not change
//! is stored once, and restores through the same path: any vault, plain or
//! encrypted, with the same ids.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Index, Item, Repo, Snapshot, WriteStats, Writer};
use crate::blobs::BlobStore;
use crate::error::{Error, Result};
use crate::id::NoteId;
use crate::store::{
    BackupAttachment, BackupData, BackupFolder, BackupNote, BackupVersion, RestoreReport, Store,
};
use crate::vault::{Encryption, Vault};

const KIND: &str = "vault";

/// One note with all that belongs to it, in a fixed order so a note that
/// did not change gives the same bytes.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NoteItem {
    note: BackupNote,
    versions: Vec<BackupVersion>,
    attachments: Vec<BackupAttachment>,
}

/// What a backup run did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunReport {
    /// The snapshot written.
    pub snapshot: String,
    /// Notes in it.
    pub notes: u64,
    /// Files in it.
    pub files: u64,
    /// Its plain size.
    pub size: u64,
    /// What was new, what was there already.
    pub stats: WriteStats,
}

fn json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| Error::Invalid(e.to_string()))
}

/// Each note with its versions and files, in a fixed order so a note that
/// did not change gives the same bytes.
fn note_items(data: &BackupData) -> Vec<NoteItem> {
    let mut versions: BTreeMap<NoteId, Vec<BackupVersion>> = BTreeMap::new();
    for v in &data.versions {
        versions.entry(v.note_id).or_default().push(v.clone());
    }
    let mut attachments: BTreeMap<NoteId, Vec<BackupAttachment>> = BTreeMap::new();
    for a in &data.attachments {
        attachments.entry(a.note_id).or_default().push(a.clone());
    }
    data.notes
        .iter()
        .map(|note| {
            let mut v = versions.remove(&note.id).unwrap_or_default();
            v.sort_by(|a, b| {
                a.created_at
                    .cmp(&b.created_at)
                    .then_with(|| a.id.cmp(&b.id))
            });
            let mut a = attachments.remove(&note.id).unwrap_or_default();
            a.sort_by(|x, y| x.id.cmp(&y.id));
            let mut note = note.clone();
            note.tags.sort();
            NoteItem {
                note,
                versions: v,
                attachments: a,
            }
        })
        .collect()
}

/// Backs the vault up into `repo`: a new snapshot, holding only what the
/// repository did not have stored. Takes a shared lock for the run.
///
/// # Errors
///
/// The repository is being pruned, database, file and target errors.
pub fn backup_vault(
    repo: &Repo,
    store: &Store,
    blobs: &BlobStore,
    host: &str,
    now: &dyn Fn() -> i64,
) -> Result<RunReport> {
    backup_data(repo, &store.backup_data()?, blobs, host, now)
}

/// As [`backup_vault`], from what the store gave: the store is needed only
/// a moment, the upload runs without it.
///
/// # Errors
///
/// As [`backup_vault`].
pub fn backup_data(
    repo: &Repo,
    data: &BackupData,
    blobs: &BlobStore,
    host: &str,
    now: &dyn Fn() -> i64,
) -> Result<RunReport> {
    let mut lock = repo.lock(host, false, now())?;
    let result = run(repo, data, blobs, host, now, &mut lock);
    let released = lock.release();
    let report = result?;
    released?;
    Ok(report)
}

fn run(
    repo: &Repo,
    data: &BackupData,
    blobs: &BlobStore,
    host: &str,
    now: &dyn Fn() -> i64,
    lock: &mut app_repo::Lock<'_>,
) -> Result<RunReport> {
    let index = repo.index()?;
    // files are named by their hash, never change: one the last snapshot of
    // this host holds is taken as it is, without reading it again
    let previous: BTreeMap<String, Item> = repo
        .snapshots()?
        .into_iter()
        .rev()
        .find(|s| s.kind == KIND && s.host == host)
        .map(|s| {
            s.items
                .into_iter()
                .filter(|i| i.name.starts_with("files/"))
                .map(|i| (i.name.clone(), i))
                .collect()
        })
        .unwrap_or_default();

    let mut writer = Writer::new(repo, &index);
    let mut items = Vec::new();
    let mut add = |writer: &mut Writer<'_>, name: String, bytes: &[u8]| -> Result<()> {
        let mut source = bytes;
        let (objects, size) = writer.add_stream(&mut source)?;
        items.push(Item {
            name,
            size,
            objects,
        });
        Ok(())
    };

    let mut folders = data.folders.clone();
    folders.sort_by_key(|f| f.id.to_string());
    add(&mut writer, "folders".into(), &json(&folders)?)?;

    for item in note_items(data) {
        add(
            &mut writer,
            format!("notes/{}", item.note.id),
            &json(&item)?,
        )?;
    }

    let mut files = 0u64;
    let mut hashes: Vec<String> = data.file_hashes().into_iter().collect();
    hashes.sort();
    for sha in hashes {
        let name = format!("files/{sha}");
        if let Some(item) = previous.get(&name)
            && item.objects.iter().all(|o| writer.has(o))
        {
            items.push(item.clone());
            files += 1;
            continue;
        }
        let Some(mut reader) = blobs.open(&sha)? else {
            // never fetched on this device: its description is in the
            // note, its content is not
            continue;
        };
        let (objects, size) = writer.add_stream(&mut reader)?;
        items.push(Item {
            name,
            size,
            objects,
        });
        files += 1;
        lock.refresh(now())?;
    }

    let stats = writer.finish()?;
    let mut counts = BTreeMap::new();
    counts.insert("notes".to_owned(), data.notes.len() as u64);
    counts.insert("folders".to_owned(), data.folders.len() as u64);
    counts.insert("versions".to_owned(), data.versions.len() as u64);
    counts.insert("files".to_owned(), files);
    let mut snapshot = Snapshot {
        id: String::new(),
        time: now(),
        host: host.to_owned(),
        kind: KIND.to_owned(),
        app_version: app_brand::VERSION.to_owned(),
        items,
        counts,
    };
    repo.save_snapshot(&mut snapshot)?;
    Ok(RunReport {
        notes: data.notes.len() as u64,
        files,
        size: snapshot.size(),
        snapshot: snapshot.id,
        stats,
    })
}

/// An error from inside an [`ItemReader`], which `Read` turned into a file
/// error: damage stays damage.
fn from_reader(e: Error) -> Error {
    if let Error::Io(io) = &e
        && let Some(inner) = io
            .get_ref()
            .and_then(|x| x.downcast_ref::<app_repo::Error>())
    {
        return match inner {
            app_repo::Error::Corrupt(why) => Error::Corrupt(why.clone()),
            app_repo::Error::NotFound(what) => Error::Corrupt(format!("{what} is missing")),
            app_repo::Error::Target(why) => Error::Target(why.clone()),
            other => Error::Io(std::io::Error::other(other.to_string())),
        };
    }
    e
}

/// Reads an item's content as it is asked for, one object at a time.
struct ItemReader<'a> {
    repo: &'a Repo,
    index: &'a Index,
    objects: std::slice::Iter<'a, String>,
    buffer: Vec<u8>,
    at: usize,
}

impl Read for ItemReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.at == self.buffer.len() {
            let Some(id) = self.objects.next() else {
                return Ok(0);
            };
            self.buffer = self
                .repo
                .read_object(self.index, id)
                .map_err(std::io::Error::other)?;
            self.at = 0;
        }
        let n = out.len().min(self.buffer.len() - self.at);
        out[..n].copy_from_slice(&self.buffer[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

fn read_all(repo: &Repo, index: &Index, item: &Item) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(usize::try_from(item.size).unwrap_or(0));
    repo.read_item(index, item, &mut out)?;
    Ok(out)
}

/// What a snapshot holds of the store.
fn snapshot_data(repo: &Repo, index: &Index, snapshot: &Snapshot) -> Result<BackupData> {
    if snapshot.kind != KIND {
        return Err(Error::Invalid(
            "this snapshot is not of an app's notes".into(),
        ));
    }
    let mut data = BackupData::default();
    let folders = snapshot
        .item("folders")
        .ok_or_else(|| Error::Corrupt("the snapshot has no folders".into()))?;
    data.folders = serde_json::from_slice::<Vec<BackupFolder>>(&read_all(repo, index, folders)?)
        .map_err(|e| Error::Corrupt(format!("folders: {e}")))?;
    for item in snapshot
        .items
        .iter()
        .filter(|i| i.name.starts_with("notes/"))
    {
        let note: NoteItem = serde_json::from_slice(&read_all(repo, index, item)?)
            .map_err(|e| Error::Corrupt(format!("{}: {e}", item.name)))?;
        data.notes.push(note.note);
        data.versions.extend(note.versions);
        data.attachments.extend(note.attachments);
    }
    Ok(data)
}

/// Puts a snapshot back into the vault: files first (each checked against
/// its hash), then folders and notes with their ids. Notes already here
/// stay as they are, restoring twice changes nothing.
///
/// # Errors
///
/// A damaged repository, database, file and target errors.
pub fn restore_vault(
    repo: &Repo,
    snapshot_id: &str,
    store: &mut Store,
    blobs: &BlobStore,
) -> Result<RestoreReport> {
    let contents = read_snapshot(repo, snapshot_id)?;
    let present = store.notes_present(&contents.note_ids())?;
    fetch_files(repo, &contents, &present, blobs)?;
    let has = |sha: &str| blobs.has(sha).unwrap_or(false);
    store.restore_backup(&contents.data, &has)
}

/// A snapshot read for a restore: what it holds of the store.
pub struct SnapshotContents {
    snapshot: Snapshot,
    index: Index,
    /// Folders, notes, versions, file descriptions.
    pub data: BackupData,
}

impl SnapshotContents {
    /// The ids of its notes, to ask the store which are here already.
    pub fn note_ids(&self) -> Vec<NoteId> {
        self.data.notes.iter().map(|n| n.id).collect()
    }
}

/// Reads a snapshot's notes and folders, without the store.
///
/// # Errors
///
/// A damaged repository, target errors.
pub fn read_snapshot(repo: &Repo, snapshot_id: &str) -> Result<SnapshotContents> {
    let snapshot = repo.snapshot(snapshot_id)?;
    let index = repo.index()?;
    let data = snapshot_data(repo, &index, &snapshot)?;
    Ok(SnapshotContents {
        snapshot,
        index,
        data,
    })
}

/// Brings into `blobs` the files of the notes not `present`, each checked
/// against its hash, without the store. Then the store's
/// `restore_backup` puts the notes back.
///
/// # Errors
///
/// A damaged repository, file and target errors.
pub fn fetch_files(
    repo: &Repo,
    contents: &SnapshotContents,
    present: &std::collections::HashSet<NoteId, impl std::hash::BuildHasher>,
    blobs: &BlobStore,
) -> Result<()> {
    let (data, snapshot, index) = (&contents.data, &contents.snapshot, &contents.index);
    let mut needed: Vec<String> = data
        .attachments
        .iter()
        .filter(|a| !present.contains(&a.note_id))
        .flat_map(|a| std::iter::once(a.sha256.clone()).chain(a.thumb_sha256.clone()))
        .collect();
    needed.sort();
    needed.dedup();
    for sha in &needed {
        if blobs.has(sha)? {
            continue;
        }
        let Some(item) = snapshot.item(&format!("files/{sha}")) else {
            // the device that made it never had it
            continue;
        };
        let mut reader = ItemReader {
            repo,
            index,
            objects: item.objects.iter(),
            buffer: Vec::new(),
            at: 0,
        };
        blobs.put_known(sha, &mut reader).map_err(from_reader)?;
    }
    Ok(())
}

/// What a test restore found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRestore {
    /// The snapshot tried.
    pub snapshot: String,
    /// What came back.
    pub restored: RestoreReport,
    /// What the snapshot says it holds, that did not come back.
    pub problems: Vec<String>,
}

/// Restores a snapshot into a scratch vault in `scratch` (which must not
/// exist), compares what came back with what the snapshot holds, and
/// removes the scratch vault.
///
/// # Errors
///
/// File and target errors. A snapshot that does not restore whole is
/// reported, not returned.
pub fn test_restore(repo: &Repo, snapshot_id: &str, scratch: &Path) -> Result<TestRestore> {
    let snapshot = repo.snapshot(snapshot_id)?;
    let mut report = TestRestore {
        snapshot: snapshot.id.clone(),
        ..TestRestore::default()
    };
    let (vault, mut store) = Vault::create(scratch, Encryption::Off)?;
    let restored = vault
        .blobs(&mut store)
        .and_then(|blobs| restore_vault(repo, snapshot_id, &mut store, &blobs));
    drop(store);
    let removed = Vault::destroy(scratch);
    match restored {
        Ok(restored) => {
            let count = |name: &str| snapshot.counts.get(name).copied().unwrap_or(0);
            let compare = [
                ("notes", restored.notes as u64),
                ("folders", restored.folders as u64),
                ("versions", restored.versions as u64),
                ("files", restored.files as u64),
            ];
            for (name, got) in compare {
                if got != count(name) {
                    report
                        .problems
                        .push(format!("{name}: {got} came back of {}", count(name)));
                }
            }
            report.restored = restored;
        }
        // whatever stopped it, a test restore says so rather than fails
        Err(e) => report.problems.push(e.to_string()),
    }
    removed?;
    Ok(report)
}
