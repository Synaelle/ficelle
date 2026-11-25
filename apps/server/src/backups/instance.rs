// SPDX-License-Identifier: AGPL-3.0-or-later

//! What an instance's snapshot holds, and putting it back. Items:
//!
//! - `server.db`: the database, copied with `VACUUM INTO` (consistent,
//!   compact), so the snapshot never catches it halfway through a write.
//! - Never the feedback reports: they go after the keeping time, a backup
//!   would keep them. Older snapshots' `feedback.db` is left out when
//!   restoring.
//! - `master.key`: the key that wraps the standard accounts' space keys
//!   and seals their files at rest. Without it, a restored instance could
//!   open none of them.
//! - `files/<object>`: every stored file, read through the file store. An
//!   object never changes under its name: one the last snapshot holds with
//!   the same size is taken as it is, without reading it again.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::Path;

use app_repo::{Index, Item, Repo, Snapshot, WriteStats, Writer};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::files::Storage;

const KIND: &str = "server";
/// The host name of an instance's snapshots.
pub const HOST: &str = "server";

/// Errors of a backup, in words for the admin.
pub type Result<T> = std::result::Result<T, String>;

#[expect(clippy::needless_pass_by_value, reason = "used as map_err(repo_error)")]
fn repo_error(e: app_repo::Error) -> String {
    e.to_string()
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunReport {
    /// The snapshot written.
    pub snapshot: String,
    /// People in it.
    pub users: u64,
    /// Stored files in it.
    pub files: u64,
    /// Its plain size.
    pub size: u64,
    /// What was new, what was there already.
    pub stats: WriteStats,
}

fn count(conn: &Connection, table: &str) -> u64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
        r.get::<_, i64>(0)
    })
    .ok()
    .and_then(|n| u64::try_from(n).ok())
    .unwrap_or(0)
}

/// Backs the instance up: `db_copy` is the database already copied by
/// `VACUUM INTO`, `master_key` the key file's path, `files` the store.
///
/// # Errors
///
/// The repository is being pruned, file and target errors.
pub fn backup(
    repo: &Repo,
    db_copy: &Path,
    master_key: Option<&Path>,
    files: &dyn Storage,
    now: &dyn Fn() -> i64,
) -> Result<RunReport> {
    let mut lock = repo.lock(HOST, false, now()).map_err(repo_error)?;
    let result = run(repo, db_copy, master_key, files, now, &mut lock);
    let released = lock.release();
    let report = result?;
    released.map_err(repo_error)?;
    Ok(report)
}

fn run(
    repo: &Repo,
    db_copy: &Path,
    master_key: Option<&Path>,
    files: &dyn Storage,
    now: &dyn Fn() -> i64,
    lock: &mut app_repo::Lock<'_>,
) -> Result<RunReport> {
    let index = repo.index().map_err(repo_error)?;
    let previous: HashMap<String, Item> = repo
        .snapshots()
        .map_err(repo_error)?
        .into_iter()
        .rev()
        .find(|s| s.kind == KIND)
        .map_or_default(|s| s.items.into_iter().map(|i| (i.name.clone(), i)).collect());
    let mut writer = Writer::new(repo, &index);
    let mut items = Vec::new();
    let add = |writer: &mut Writer<'_>,
               items: &mut Vec<Item>,
               name: &str,
               source: &mut dyn Read|
     -> Result<()> {
        let (objects, size) = writer.add_stream(source).map_err(repo_error)?;
        items.push(Item {
            name: name.to_owned(),
            size,
            objects,
        });
        Ok(())
    };
    let mut db = std::fs::File::open(db_copy).map_err(|e| e.to_string())?;
    add(&mut writer, &mut items, "server.db", &mut db)?;
    if let Some(key) = master_key {
        let mut file = std::fs::File::open(key).map_err(|e| format!("{}: {e}", key.display()))?;
        add(&mut writer, &mut items, "master.key", &mut file)?;
    }
    let conn = Connection::open_with_flags(db_copy, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let objects: Vec<(String, u64)> = {
        let mut stmt = conn
            .prepare("SELECT object, size FROM files ORDER BY object")
            .map_err(|e| e.to_string())?;
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|e| e.to_string())?
            .map(|row| row.map(|(o, s)| (o, u64::try_from(s).unwrap_or(0))))
            .collect::<std::result::Result<_, rusqlite::Error>>()
            .map_err(|e| e.to_string())?
    };
    let mut stored = 0u64;
    for (object, _) in &objects {
        let name = format!("files/{object}");
        if let Some(item) = previous.get(&name)
            && item.objects.iter().all(|o| writer.has(o))
        {
            items.push(item.clone());
            stored += 1;
            continue;
        }
        let mut reader = match files.read(object, 0) {
            Ok(reader) => reader,
            // a file let go between the copy and now: not in this snapshot
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("file {object}: {e}")),
        };
        add(&mut writer, &mut items, &name, &mut reader)?;
        stored += 1;
        lock.refresh(now()).map_err(repo_error)?;
    }
    let stats = writer.finish().map_err(repo_error)?;
    let mut counts = BTreeMap::new();
    counts.insert("users".to_owned(), count(&conn, "users"));
    counts.insert("spaces".to_owned(), count(&conn, "sync_spaces"));
    counts.insert("files".to_owned(), stored);
    let mut snapshot = Snapshot {
        id: String::new(),
        time: now(),
        host: HOST.to_owned(),
        kind: KIND.to_owned(),
        app_version: app_brand::VERSION.to_owned(),
        items,
        counts,
    };
    repo.save_snapshot(&mut snapshot).map_err(repo_error)?;
    Ok(RunReport {
        users: snapshot.counts["users"],
        files: stored,
        size: snapshot.size(),
        snapshot: snapshot.id,
        stats,
    })
}

fn server_snapshot(repo: &Repo, id: &str) -> Result<Snapshot> {
    let snapshot = repo.snapshot(id).map_err(repo_error)?;
    if snapshot.kind != KIND {
        return Err("this snapshot is not of an instance".into());
    }
    Ok(snapshot)
}

/// Writes an item into a file, beside then in place.
fn write_item(repo: &Repo, index: &Index, item: &Item, to: &Path) -> Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let partial = to.with_extension("partial");
    let mut file = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    repo.read_item(index, item, &mut file).map_err(repo_error)?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(&partial, to).map_err(|e| e.to_string())
}

/// Puts an object back into a file store, through an upload as any file.
fn put_object(
    repo: &Repo,
    index: &Index,
    item: &Item,
    object: &str,
    files: &dyn Storage,
) -> Result<()> {
    let upload = format!("restore-{object}");
    let mut offset = 0u64;
    for id in &item.objects {
        let bytes = repo.read_object(index, id).map_err(repo_error)?;
        files
            .write_incoming(&upload, offset, &bytes)
            .map_err(|e| e.to_string())?;
        offset += bytes.len() as u64;
    }
    if offset != item.size {
        let _ = files.discard(&upload);
        return Err(format!("{} is not the size it was", item.name));
    }
    files.complete(&upload, object).map_err(|e| e.to_string())
}

/// Restores a whole instance into `data_dir`, which must be empty: the
/// database, the master key, every file. For a server that is stopped.
///
/// # Errors
///
/// A folder that is not empty, a damaged repository, file and target
/// errors.
pub fn restore(repo: &Repo, snapshot_id: &str, data_dir: &Path) -> Result<Snapshot> {
    if data_dir.exists()
        && std::fs::read_dir(data_dir)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err(format!(
            "{} is not empty: restore into an empty folder",
            data_dir.display()
        ));
    }
    let snapshot = server_snapshot(repo, snapshot_id)?;
    let index = repo.index().map_err(repo_error)?;
    let files = crate::files::LocalDisk::new(data_dir.join("files"));
    for item in &snapshot.items {
        match item.name.as_str() {
            "server.db" => write_item(repo, &index, item, &data_dir.join(crate::db::DB_FILE))?,
            "master.key" => write_item(repo, &index, item, &data_dir.join(crate::keys::KEY_FILE))?,
            name => {
                let Some(object) = name.strip_prefix("files/") else {
                    continue;
                };
                put_object(repo, &index, item, object, &files)?;
            }
        }
    }
    Ok(snapshot)
}

/// Where a restore from the setup page waits for the next start, in the
/// data folder.
pub const RESTORED: &str = "restored";

/// Restores the newest snapshot for a fresh server, from its setup page:
/// into `scratch`, then to [`RESTORED`] in `data_dir`, where the next start
/// puts it in place. `key_apart` says the master key is read from a file of
/// its own, so a snapshot without it restores.
///
/// # Errors
///
/// No snapshot of an instance, a snapshot made with the master key kept
/// apart on a server that keeps it in the data folder, restore errors.
pub fn restore_for_start(
    repo: &Repo,
    scratch: &Path,
    data_dir: &Path,
    key_apart: bool,
) -> Result<Snapshot> {
    let newest = repo
        .snapshots()
        .map_err(repo_error)?
        .into_iter()
        .rfind(|s| s.kind == KIND)
        .ok_or("there is no backup of an instance there")?;
    if newest.item("master.key").is_none() && !key_apart {
        return Err("this backup was made with the master key kept apart: set \
             storage.master_key_file to that key, restart the server, then restore"
            .into());
    }
    let result = restore(repo, &newest.id, scratch).and_then(|snapshot| {
        let to = data_dir.join(RESTORED);
        if to.exists() {
            std::fs::remove_dir_all(&to).map_err(|e| e.to_string())?;
        }
        std::fs::rename(scratch, &to).map_err(|e| e.to_string())?;
        Ok(snapshot)
    });
    if result.is_err() && scratch.exists() {
        let _ = std::fs::remove_dir_all(scratch);
    }
    result
}

/// At start, before the databases open: a restore from the setup page put
/// in place of the fresh instance it was made on. Returns whether there was
/// one. A database that was set up since is never replaced.
///
/// # Errors
///
/// The data folder was set up meanwhile, file errors.
pub fn take_restored(data_dir: &Path) -> Result<bool> {
    let restored = data_dir.join(RESTORED);
    if !restored.join(crate::db::DB_FILE).is_file() {
        return Ok(false);
    }
    let current = data_dir.join(crate::db::DB_FILE);
    if current.exists() {
        let conn = Connection::open_with_flags(&current, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        if crate::setup::is_locked(&conn).map_err(|e| e.to_string())? {
            return Err(format!(
                "this server was set up after the backup was restored, so it stays as it is. \
                 Remove {} to start without the restored backup",
                restored.display()
            ));
        }
    }
    for name in ["server.db-wal", "server.db-shm"] {
        match std::fs::remove_file(data_dir.join(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
    }
    for entry in std::fs::read_dir(&restored).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let to = data_dir.join(entry.file_name());
        if to.is_dir() {
            std::fs::remove_dir_all(&to).map_err(|e| e.to_string())?;
        } else if to.exists() {
            std::fs::remove_file(&to).map_err(|e| e.to_string())?;
        }
        std::fs::rename(entry.path(), &to).map_err(|e| e.to_string())?;
    }
    std::fs::remove_dir(&restored).map_err(|e| e.to_string())?;
    Ok(true)
}

/// What a test restore found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRestore {
    /// The snapshot tried.
    pub snapshot: String,
    /// People in the restored database.
    pub users: u64,
    /// Files restored.
    pub files: u64,
    /// What did not come back as the snapshot says, in words.
    pub problems: Vec<String>,
}

/// Restores a snapshot into `scratch` (made, then removed), opens the
/// database, checks it and compares what came back.
///
/// # Errors
///
/// Target errors. What does not restore is reported, not returned.
pub fn test_restore(repo: &Repo, snapshot_id: &str, scratch: &Path) -> Result<TestRestore> {
    let mut report = TestRestore {
        snapshot: snapshot_id.to_owned(),
        ..TestRestore::default()
    };
    match restore(repo, snapshot_id, scratch) {
        Ok(snapshot) => {
            let db = scratch.join(crate::db::DB_FILE);
            match Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY) {
                Ok(conn) => {
                    let check: String = conn
                        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                        .unwrap_or_else(|e| e.to_string());
                    if check != "ok" {
                        report.problems.push(format!("the database: {check}"));
                    }
                    report.users = count(&conn, "users");
                    let want = snapshot.counts.get("users").copied().unwrap_or(0);
                    if report.users != want {
                        report
                            .problems
                            .push(format!("people: {} came back of {want}", report.users));
                    }
                }
                Err(e) => report.problems.push(format!("the database: {e}")),
            }
            report.files = snapshot
                .items
                .iter()
                .filter(|i| i.name.starts_with("files/"))
                .filter(|i| {
                    let object = &i.name["files/".len()..];
                    let disk = crate::files::LocalDisk::new(scratch.join("files"));
                    disk.read(object, 0)
                        .ok()
                        .map(|mut r| std::io::copy(&mut r, &mut std::io::sink()).unwrap_or(0))
                        == Some(i.size)
                })
                .count() as u64;
            let want = snapshot.counts.get("files").copied().unwrap_or(0);
            if report.files != want {
                report
                    .problems
                    .push(format!("files: {} came back of {want}", report.files));
            }
        }
        Err(e) => report.problems.push(e),
    }
    if scratch.exists() {
        std::fs::remove_dir_all(scratch).map_err(|e| e.to_string())?;
    }
    Ok(report)
}
