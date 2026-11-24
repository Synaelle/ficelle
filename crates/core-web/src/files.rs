// SPDX-License-Identifier: AGPL-3.0-or-later

//! Backups, the Markdown export and imports, on bytes: the page hands over
//! a picked file whole and saves what comes back as a download. The work
//! itself is the core's (`app_core::commands`), as in the apps.

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Cursor;

use app_core::backup::Backup;
use app_core::commands::import::{Imports, PageBatch, Stored};
use app_core::crypto::KdfParams;
use app_core::id::FolderId;
use app_core::import::source::Source;
use serde_json::Value;

use crate::commands::{Args, answer, with};
use crate::{CommandError, CommandResult, SESSION};

thread_local! {
    /// The exports open.
    static IMPORTS: RefCell<Imports> = RefCell::new(Imports::default());
    /// What the last command answering with bytes reported (a backup's
    /// manifest, an export's counts), for the page to ask after.
    static REPORT: RefCell<Value> = const { RefCell::new(Value::Null) };
    /// Files stored for an import, which its notes refer to only once
    /// written: kept from the cleanup until the worker ends.
    static PUT: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

fn no_file() -> CommandError {
    CommandError::new("invalid", "no file was sent")
}

fn backup(bytes: Option<Vec<u8>>, a: &Args) -> CommandResult<Backup> {
    let password: Option<String> = a.get("password")?;
    let bytes = bytes.ok_or_else(no_file)?;
    Ok(Backup::open_file(Cursor::new(bytes), password.as_deref())?)
}

/// Opens a picked file to import from.
fn import_open(a: &Args, bytes: Option<Vec<u8>>) -> CommandResult<Value> {
    let name: String = a.get("name")?;
    let source = Source::open_file(Cursor::new(bytes.ok_or_else(no_file)?), &name)?;
    answer(IMPORTS.with_borrow_mut(|imports| imports.open(source, name)))
}

/// Writes a batch: its files stored from the export, then its folders and
/// notes in one transaction.
fn import_write(a: &Args) -> CommandResult<Value> {
    let (id, batch): (u32, PageBatch) = (a.get("id")?, a.get("batch")?);
    let blobs = SESSION.with_borrow(|session| {
        session
            .as_ref()
            .map(|s| s.blobs.clone())
            .ok_or_else(CommandError::locked)
    })?;
    let written = IMPORTS.with_borrow_mut(|imports| {
        imports.write(
            id,
            batch,
            &blobs,
            None,
            |source, entry| source.store(entry, &blobs),
            |place, batch| {
                SESSION.with_borrow_mut(|session| {
                    let session = session.as_mut().ok_or_else(CommandError::locked)?;
                    Ok::<_, CommandError>(session.store.import_batch(place, batch)?)
                })
            },
        )
    })?;
    answer(written)
}

/// The files stored for imports, see `PUT`.
pub fn put_for_imports() -> HashSet<String> {
    PUT.with_borrow(Clone::clone)
}

/// The commands that take json and maybe bytes, `None` for another name.
pub fn run(name: &str, a: &Args, bytes: Option<Vec<u8>>) -> Option<CommandResult<Value>> {
    Some(match name {
        "last_report" => Ok(REPORT.with_borrow_mut(std::mem::take)),
        "backup_default_name" => answer(app_core::commands::backup::default_name(
            app_core::clock::Clock::now(&app_core::clock::SystemClock),
        )),
        "backup_encrypted" => bytes.ok_or_else(no_file).and_then(|bytes| {
            answer(app_core::backup::is_encrypted_file(&mut Cursor::new(
                bytes,
            ))?)
        }),
        "backup_inspect" => backup(bytes, a).and_then(|mut backup| {
            with(|s| app_core::commands::backup::inspect(&mut backup, &s.store))
        }),
        "backup_restore" => {
            backup(bytes, a).and_then(|mut backup| with(|s| backup.restore(&mut s.store, &s.blobs)))
        }
        "import_open" => import_open(a, bytes),
        "import_known" => (|| {
            let (importer, keys): (String, Vec<String>) = (a.get("importer")?, a.get("keys")?);
            with(|s| {
                Ok(s.store
                    .imported(&importer, &keys)?
                    .into_iter()
                    .collect::<Vec<_>>())
            })
        })(),
        "import_begin" => (|| {
            let (id, name): (u32, String) = (a.get("id")?, a.get("name")?);
            let root: FolderId = IMPORTS.with_borrow_mut(|imports| {
                imports.begin(id, || {
                    SESSION.with_borrow_mut(|session| {
                        let session = session.as_mut().ok_or_else(CommandError::locked)?;
                        Ok::<_, CommandError>(session.store.begin_import(&name)?)
                    })
                })
            })?;
            answer(root)
        })(),
        "import_put" => bytes.ok_or_else(no_file).and_then(|bytes| {
            with(|s| {
                let sha256 = s.blobs.put(&bytes)?;
                PUT.with_borrow_mut(|put| put.insert(sha256.clone()));
                Ok(Stored {
                    sha256,
                    size: bytes.len() as u64,
                })
            })
        }),
        "import_write" => import_write(a),
        "import_close" => a.get::<u32>("id").and_then(|id| {
            match IMPORTS.with_borrow_mut(|imports| imports.close(id)) {
                Some(root) => with(|s| s.store.end_import(root)),
                None => Ok(Value::Null),
            }
        }),
        _ => return None,
    })
}

/// The commands that answer with bytes, `None` for another name.
pub fn bytes(name: &str, a: &Args) -> Option<CommandResult<Vec<u8>>> {
    Some(match name {
        "backup_write" => (|| {
            let password: Option<String> = a.get("password")?;
            let password = password.as_deref().filter(|p| !p.is_empty());
            let mut out = Vec::new();
            let manifest = SESSION.with_borrow(|session| {
                let s = session.as_ref().ok_or_else(CommandError::locked)?;
                Ok::<_, CommandError>(app_core::backup::write_backup_to(
                    &s.store,
                    &s.blobs,
                    &mut out,
                    password,
                    KdfParams::DEFAULT,
                )?)
            })?;
            REPORT.with_borrow_mut(|report| *report = answer(manifest).unwrap_or_default());
            Ok(out)
        })(),
        "export_markdown_zip" => (|| {
            let mut out = Vec::new();
            let report = SESSION.with_borrow(|session| {
                let s = session.as_ref().ok_or_else(CommandError::locked)?;
                Ok::<_, CommandError>(app_core::export::export_markdown_zip_to(
                    &s.store, &mut out, &s.blobs,
                )?)
            })?;
            REPORT.with_borrow_mut(|done| *done = answer(report).unwrap_or_default());
            Ok(out)
        })(),
        "import_read" => (|| {
            let (id, path): (u32, String) = (a.get("id")?, a.get("path")?);
            Ok(IMPORTS.with_borrow_mut(|imports| imports.read(id, &path))?)
        })(),
        _ => return None,
    })
}
