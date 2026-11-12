// SPDX-License-Identifier: AGPL-3.0-or-later

//! The vault's files in the browser's private storage (OPFS). The databases
//! are kept by SQLite in its pool of storage files, every other file is a
//! row of a database of its own. Paths keep the folder layout.

use std::cell::RefCell;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use app_core::Result;
use app_core::clock::{Clock, SystemClock};
use app_core::disk::Disk;
use rusqlite::{Connection, OptionalExtension, params};
use sqlite_wasm_rs::vfs::VfsFilesManager;
use sqlite_wasm_rs::vfs::transfer::DbTransfer;
use sqlite_wasm_vfs::sahpool::OpfsSAHPoolUtil;

thread_local! {
    static POOL: RefCell<Option<OpfsSAHPoolUtil>> = const { RefCell::new(None) };
    static FILES: RefCell<Option<Connection>> = const { RefCell::new(None) };
}

/// The database of files that are not databases.
const FILES_DB: &str = "/files.db";

/// Keeps the pool and opens the database of files, once the pool is SQLite's.
///
/// # Errors
///
/// The database of files cannot be opened.
pub fn start(pool: OpfsSAHPoolUtil) -> Result<()> {
    let files = Connection::open(FILES_DB)?;
    files.execute_batch(
        "CREATE TABLE IF NOT EXISTS files (
            path TEXT PRIMARY KEY,
            data BLOB NOT NULL,
            modified INTEGER NOT NULL
        )",
    )?;
    POOL.with_borrow_mut(|p| *p = Some(pool));
    FILES.with_borrow_mut(|f| *f = Some(files));
    Ok(())
}

/// The vault's files, see the module.
#[derive(Debug, Clone, Copy, Default)]
pub struct WebDisk;

fn not_started() -> std::io::Error {
    std::io::Error::other("the storage is not started")
}

fn files<T>(f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> std::io::Result<T> {
    FILES.with_borrow(|files| {
        f(files.as_ref().ok_or_else(not_started)?).map_err(std::io::Error::other)
    })
}

fn pool<T, E: std::error::Error + Send + Sync + 'static>(
    f: impl FnOnce(&OpfsSAHPoolUtil) -> std::result::Result<T, E>,
) -> std::io::Result<T> {
    POOL.with_borrow(|pool| {
        f(pool.as_ref().ok_or_else(not_started)?).map_err(std::io::Error::other)
    })
}

/// The databases, which SQLite keeps itself: `notes.db` and the files
/// beside it.
fn pooled(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("notes.db"))
}

fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// What paths inside a folder start with.
fn inside(dir: &Path) -> String {
    format!("{}/", key(dir).trim_end_matches('/'))
}

fn pooled_inside(dir: &Path) -> std::io::Result<Vec<String>> {
    let prefix = inside(dir);
    Ok(pool(VfsFilesManager::names)?
        .into_iter()
        .filter(|name| name.starts_with(&prefix))
        .collect())
}

fn not_found() -> std::io::Error {
    std::io::ErrorKind::NotFound.into()
}

impl Disk for WebDisk {
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Read + Send>> {
        if pooled(path) {
            return Ok(Box::new(Cursor::new(pool(|p| p.export_db(&key(path)))?)));
        }
        let data: Option<Vec<u8>> = files(|f| {
            f.query_row(
                "SELECT data FROM files WHERE path = ?1",
                [key(path)],
                |row| row.get(0),
            )
            .optional()
        })?;
        Ok(Box::new(Cursor::new(data.ok_or_else(not_found)?)))
    }

    fn len(&self, path: &Path) -> std::io::Result<u64> {
        let len: Option<i64> = files(|f| {
            f.query_row(
                "SELECT length(data) FROM files WHERE path = ?1",
                [key(path)],
                |row| row.get(0),
            )
            .optional()
        })?;
        len.and_then(|n| u64::try_from(n).ok())
            .ok_or_else(not_found)
    }

    fn exists(&self, path: &Path) -> bool {
        if pooled(path) {
            return pool(|p| p.contains(&key(path))).unwrap_or(false);
        }
        let row = files(|f| {
            f.query_row(
                "SELECT 1 FROM files WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2 LIMIT 1",
                params![key(path), inside(path)],
                |_| Ok(()),
            )
            .optional()
        });
        matches!(row, Ok(Some(()))) || pooled_inside(path).is_ok_and(|names| !names.is_empty())
    }

    fn make_folder(&self, _dir: &Path) -> std::io::Result<()> {
        // folders are only in the paths
        Ok(())
    }

    fn write(&self, path: &Path, fill: &mut dyn FnMut(&mut dyn Write) -> Result<()>) -> Result<()> {
        let mut data = Vec::new();
        fill(&mut data)?;
        let now = SystemClock.now();
        files(|f| {
            f.execute(
                "INSERT OR REPLACE INTO files (path, data, modified) VALUES (?1, ?2, ?3)",
                params![key(path), data, now],
            )
        })?;
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if pooled(from) {
            // a database is copied over: its storage has no renaming
            let bytes = pool(|p| p.export_db(&key(from)))?;
            pool(|p| p.remove(&key(to)))?;
            pool(|p| p.import_db_unchecked(&key(to), &bytes))?;
            pool(|p| p.remove(&key(from)))?;
            return Ok(());
        }
        files(|f| {
            f.execute(
                "DELETE FROM files WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2",
                params![key(to), inside(to)],
            )?;
            f.execute(
                "UPDATE files SET path = ?3 || substr(path, length(?1) + 1)
                 WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2",
                params![key(from), inside(from), key(to)],
            )
        })?;
        Ok(())
    }

    fn remove(&self, path: &Path) -> std::io::Result<()> {
        if pooled(path) {
            pool(|p| p.remove(&key(path)))?;
            return Ok(());
        }
        files(|f| f.execute("DELETE FROM files WHERE path = ?1", [key(path)]))?;
        Ok(())
    }

    fn remove_all(&self, dir: &Path) -> std::io::Result<()> {
        files(|f| {
            f.execute(
                "DELETE FROM files WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2",
                params![key(dir), inside(dir)],
            )
        })?;
        for name in pooled_inside(dir)? {
            pool(|p| p.remove(&name))?;
        }
        Ok(())
    }

    fn list(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        let prefix = inside(dir);
        let mut paths: Vec<String> = files(|f| {
            f.prepare("SELECT path FROM files WHERE substr(path, 1, length(?1)) = ?1")?
                .query_map([&prefix], |row| row.get(0))?
                .collect()
        })?;
        paths.extend(pooled_inside(dir)?);
        let mut children: Vec<PathBuf> = paths
            .iter()
            .filter_map(|path| path[prefix.len()..].split('/').next())
            .map(|child| dir.join(child))
            .collect();
        children.sort();
        children.dedup();
        Ok(children)
    }

    fn age(&self, path: &Path) -> Option<Duration> {
        let modified: i64 = files(|f| {
            f.query_row(
                "SELECT modified FROM files WHERE path = ?1",
                [key(path)],
                |row| row.get(0),
            )
        })
        .ok()?;
        u64::try_from(SystemClock.now() - modified)
            .ok()
            .map(Duration::from_millis)
    }
}
