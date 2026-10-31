// SPDX-License-Identifier: AGPL-3.0-or-later

//! What an instance's snapshot holds, and putting it back. Items:
//!
//! - `server.db`: the database, copied with `VACUUM INTO` (consistent,
//!   compact), so the snapshot never catches it halfway through a write.
//! - `feedback.db`: the feedback reports and block lists, copied the same
//!   way, when the instance has them (beside the database's copy).
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
/// `VACUUM INTO` (the feedback database's copy beside it, when there is one), `master_key` the key file's path, `files` the store.
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
        .map(|s| s.items.into_iter().map(|i| (i.name.clone(), i)).collect())
        .unwrap_or_default();
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
    let feedback = db_copy.with_file_name(crate::db::FEEDBACK_FILE);
    if feedback.exists() {
        let mut file = std::fs::File::open(&feedback).map_err(|e| e.to_string())?;
        add(&mut writer, &mut items, "feedback.db", &mut file)?;
    }
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
            .filter_map(std::result::Result::ok)
            .map(|(o, s)| (o, u64::try_from(s).unwrap_or(0)))
            .collect()
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
            "feedback.db" => {
                write_item(repo, &index, item, &data_dir.join(crate::db::FEEDBACK_FILE))?;
            }
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

/// What came back of a person.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonReport {
    /// Their account came back (it was gone).
    pub account: bool,
    /// Their spaces that came back.
    pub spaces: u64,
    /// Changes of those spaces.
    pub changes: u64,
    /// Files put back in the store.
    pub files: u64,
}

/// Restores the snapshot's database into `scratch` and brings it to the
/// running version, for [`restore_person`]. Returns its path.
///
/// # Errors
///
/// As [`restore`].
pub fn snapshot_database(
    repo: &Repo,
    snapshot_id: &str,
    scratch: &Path,
) -> Result<std::path::PathBuf> {
    let snapshot = server_snapshot(repo, snapshot_id)?;
    let index = repo.index().map_err(repo_error)?;
    let item = snapshot
        .item("server.db")
        .ok_or("the snapshot has no database")?;
    let path = scratch.join(crate::db::DB_FILE);
    write_item(repo, &index, item, &path)?;
    // an older snapshot gets the tables this version has
    drop(crate::db::Db::open(scratch).map_err(|e| e.to_string())?);
    Ok(path)
}

/// The person's id in the snapshot and the files their missing spaces
/// need that the store does not have.
///
/// # Errors
///
/// No such person in the snapshot, the name taken by someone else since.
pub fn person_plan(
    live: &Connection,
    snapshot_db: &Path,
    username: &str,
) -> Result<(String, Vec<String>)> {
    live.execute(
        "ATTACH DATABASE ?1 AS snap",
        [snapshot_db.to_string_lossy()],
    )
    .map_err(|e| e.to_string())?;
    let plan = (|| {
        let id: String = live
            .query_row(
                "SELECT id FROM snap.users WHERE username = ?1 COLLATE NOCASE",
                [username],
                |r| r.get(0),
            )
            .map_err(|_| format!("{username} is not in this backup"))?;
        let taken: Option<String> = live
            .query_row(
                "SELECT id FROM main.users WHERE username = ?1 COLLATE NOCASE AND id <> ?2",
                [username, id.as_str()],
                |r| r.get(0),
            )
            .ok();
        if taken.is_some() {
            return Err(format!(
                "{username} is the name of another account now: rename it first"
            ));
        }
        let mut stmt = live
            .prepare(
                "SELECT DISTINCT r.object FROM snap.file_refs r
                 JOIN snap.sync_spaces s ON s.id = r.space_id
                 WHERE s.owner_id = ?1
                   AND s.id NOT IN (SELECT id FROM main.sync_spaces)
                   AND r.object NOT IN (SELECT object FROM main.files)",
            )
            .map_err(|e| e.to_string())?;
        let objects = stmt
            .query_map([&id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(std::result::Result::ok)
            .collect();
        Ok((id, objects))
    })();
    let _ = live.execute("DETACH DATABASE snap", []);
    plan
}

/// Puts the files of a plan back into the store, before the rows that
/// name them.
///
/// # Errors
///
/// A damaged repository, file and target errors.
pub fn restore_objects(
    repo: &Repo,
    snapshot_id: &str,
    objects: &[String],
    files: &dyn Storage,
) -> Result<u64> {
    let snapshot = server_snapshot(repo, snapshot_id)?;
    let index = repo.index().map_err(repo_error)?;
    let mut done = 0;
    for object in objects {
        let Some(item) = snapshot.item(&format!("files/{object}")) else {
            continue;
        };
        put_object(repo, &index, item, object, files)?;
        done += 1;
    }
    Ok(done)
}

/// What is missing of a person comes back from the snapshot's database:
/// their account if it is gone (devices and sign-ins stay gone, they sign
/// in again), their spaces that are gone with what they hold, their
/// memberships of standard spaces still here. Nothing here is changed.
/// Changes get new sequence numbers, after everything here.
///
/// An end-to-end encrypted space still here is not opened to them again:
/// its owner may have removed them since the snapshot, and only the owner
/// lets people in.
///
/// # Errors
///
/// Database errors (the transaction then changes nothing).
pub fn restore_person(
    live: &mut Connection,
    snapshot_db: &Path,
    user: &str,
) -> Result<PersonReport> {
    live.execute(
        "ATTACH DATABASE ?1 AS snap",
        [snapshot_db.to_string_lossy()],
    )
    .map_err(|e| e.to_string())?;
    let result = (|| -> rusqlite::Result<PersonReport> {
        let tx = live.transaction()?;
        let mut report = PersonReport {
            account: tx.execute(
                "INSERT OR IGNORE INTO main.users SELECT * FROM snap.users WHERE id = ?1",
                [user],
            )? > 0,
            ..PersonReport::default()
        };
        for table in ["recovery_codes", "passkeys", "sso_identities"] {
            tx.execute(
                &format!("INSERT OR IGNORE INTO main.{table} SELECT * FROM snap.{table} WHERE user_id = ?1"),
                [user],
            )?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO main.group_members SELECT * FROM snap.group_members
             WHERE user_id = ?1 AND group_id IN (SELECT id FROM main.groups)",
            [user],
        )?;
        tx.execute(
            "CREATE TEMP TABLE back AS SELECT id FROM snap.sync_spaces
             WHERE owner_id = ?1 AND id NOT IN (SELECT id FROM main.sync_spaces)",
            [user],
        )?;
        report.spaces = u64::try_from(tx.execute(
            "INSERT INTO main.sync_spaces SELECT * FROM snap.sync_spaces WHERE id IN (SELECT id FROM temp.back)",
            [],
        )?)
        .unwrap_or(0);
        report.changes = u64::try_from(tx.execute(
            "INSERT INTO main.sync_changes
                 (space_id, doc_id, device_id, change_id, bytes, size, created_at, compacted)
             SELECT space_id, doc_id,
                    CASE WHEN device_id IN (SELECT id FROM main.devices) THEN device_id END,
                    change_id, bytes, size, created_at, compacted
             FROM snap.sync_changes WHERE space_id IN (SELECT id FROM temp.back) ORDER BY seq",
            [],
        )?)
        .unwrap_or(0);
        tx.execute(
            "INSERT OR IGNORE INTO main.sync_doc_keys SELECT * FROM snap.sync_doc_keys
             WHERE space_id IN (SELECT id FROM temp.back)",
            [],
        )?;
        // members of the spaces back, and the person's memberships of the
        // standard spaces still here, as long as who they name still exists
        tx.execute(
            "INSERT OR IGNORE INTO main.sync_members SELECT * FROM snap.sync_members m
             WHERE (m.space_id IN (SELECT id FROM temp.back)
                    OR (m.user_id = ?1 AND m.space_id IN
                        (SELECT id FROM main.sync_spaces WHERE kind = 'standard')))
               AND m.space_id IN (SELECT id FROM main.sync_spaces)
               AND (m.user_id IS NULL OR m.user_id IN (SELECT id FROM main.users))
               AND (m.group_id IS NULL OR m.group_id IN (SELECT id FROM main.groups))",
            [user],
        )?;
        for table in ["sync_left", "sync_grants"] {
            tx.execute(
                &format!(
                    "INSERT OR IGNORE INTO main.{table} SELECT * FROM snap.{table} t
                     WHERE (t.space_id IN (SELECT id FROM temp.back)
                            OR (t.user_id = ?1 AND t.space_id IN
                                (SELECT id FROM main.sync_spaces WHERE kind = 'standard')))
                       AND t.space_id IN (SELECT id FROM main.sync_spaces)
                       AND t.user_id IN (SELECT id FROM main.users)"
                ),
                [user],
            )?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO main.files SELECT * FROM snap.files WHERE object IN
                 (SELECT object FROM snap.file_refs WHERE space_id IN (SELECT id FROM temp.back))",
            [],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO main.file_refs SELECT * FROM snap.file_refs
             WHERE space_id IN (SELECT id FROM temp.back)",
            [],
        )?;
        tx.execute("DROP TABLE temp.back", [])?;
        tx.commit()?;
        Ok(report)
    })();
    let _ = live.execute("DETACH DATABASE snap", []);
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use app_protocol::account::Role;

    use super::*;
    use crate::accounts::store;
    use crate::db::{DB_FILE, Db};

    /// The same people and spaces in both: alex, owner of an e2ee and a
    /// standard space, and kim.
    async fn people(db: &Db) -> String {
        db.call(|c| {
            let alex = store::insert_user_with_id(c, "a", "alex", "Alex", "h", Role::User, 1)?;
            store::insert_user_with_id(c, "k", "kim", "Kim", "h", Role::User, 1)?;
            for (id, kind) in [("e", "e2ee"), ("s", "standard")] {
                c.execute(
                    "INSERT INTO sync_spaces (id, owner_id, created_at, shared, kind)
                     VALUES (?1, 'a', 1, 1, ?2)",
                    [id, kind],
                )?;
            }
            Ok(alex.unwrap().id)
        })
        .await
        .unwrap()
    }

    /// A person restored is not let back into
    /// an e2ee space the owner may have removed them from.
    #[tokio::test]
    async fn a_person_restored_comes_back_to_standard_spaces_only() {
        let snap = tempfile::tempdir().unwrap();
        let db = Db::open(snap.path()).unwrap();
        people(&db).await;
        db.call(|c| {
            for space in ["e", "s"] {
                c.execute(
                    "INSERT INTO sync_members (space_id, user_id, role, added_at)
                     VALUES (?1, 'k', 'edit', 1)",
                    [space],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();
        drop(db);

        let live = Db::in_memory().unwrap();
        people(&live).await;
        let path = snap.path().join(DB_FILE);
        let members = live
            .call(move |c| {
                restore_person(c, &path, "k").map_err(|_| rusqlite::Error::InvalidQuery)?;
                let mut stmt =
                    c.prepare("SELECT space_id FROM sync_members WHERE user_id = 'k'")?;
                Ok(stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .await
            .unwrap();
        assert_eq!(members, vec!["s".to_string()]);
    }
}
