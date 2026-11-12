// SPDX-License-Identifier: AGPL-3.0-or-later

//! Backups, the Markdown export and imports, on bytes: the page hands over
//! a picked file whole and saves what comes back as a download. The same
//! work as the apps' (`src-tauri/src/commands/backup.rs` and `import.rs`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;

use app_core::backup::{Backup, Manifest};
use app_core::crypto::KdfParams;
use app_core::id::{FolderId, NoteId};
use app_core::import::source::{Entry, Source};
use app_core::store::{ImportBatch, ImportFile, ImportFolder, ImportNote, ImportPlace};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{Args, answer, with};
use crate::{CommandError, CommandResult, SESSION};

/// An export being imported.
struct Open {
    source: Source,
    place: Option<ImportPlace>,
}

thread_local! {
    /// The exports open, by the number the page knows them by.
    static IMPORTS: RefCell<(u32, HashMap<u32, Open>)> = RefCell::new((0, HashMap::new()));
    /// What the last command answering with bytes reported (a backup's
    /// manifest, an export's counts), for the page to ask after.
    static REPORT: RefCell<Value> = const { RefCell::new(Value::Null) };
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportSummary {
    notes: usize,
    folders: usize,
    attachments: usize,
    missing_attachments: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Inspected {
    manifest: Manifest,
    present: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Opened {
    id: u32,
    name: String,
    entries: Vec<Entry>,
    dirs: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    sha256: String,
    size: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageFile {
    name: String,
    mime: String,
    entry: Option<String>,
    sha256: Option<String>,
    size: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageNote {
    #[serde(flatten)]
    note: ImportNote,
    #[serde(default)]
    sources: Vec<PageFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageBatch {
    importer: String,
    #[serde(default)]
    folders: Vec<ImportFolder>,
    notes: Vec<PageNote>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LeftOut {
    note: String,
    name: String,
    why: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Written {
    notes: Vec<(String, NoteId)>,
    left_out: Vec<LeftOut>,
}

fn import<T>(id: u32, f: impl FnOnce(&mut Open) -> CommandResult<T>) -> CommandResult<T> {
    IMPORTS.with_borrow_mut(|(_, open)| {
        let item = open
            .get_mut(&id)
            .ok_or_else(|| CommandError::new("notFound", "this import is closed"))?;
        f(item)
    })
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
    let entries = source.entries().to_vec();
    let dirs = source.dirs().to_vec();
    let id = IMPORTS.with_borrow_mut(|(next, open)| {
        let id = *next;
        *next += 1;
        open.insert(
            id,
            Open {
                source,
                place: None,
            },
        );
        id
    });
    answer(Opened {
        id,
        name,
        entries,
        dirs,
    })
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
    import(id, |open| {
        let mut left_out = Vec::new();
        let mut notes = Vec::with_capacity(batch.notes.len());
        for PageNote { mut note, sources } in batch.notes {
            for file in sources {
                let stored = match (&file.entry, &file.sha256) {
                    (Some(entry), _) => match open.source.store(entry, &blobs) {
                        Ok(stored) => Some(stored),
                        Err(app_core::Error::NotFound(_)) => {
                            left_out.push(LeftOut {
                                note: note.title.clone(),
                                name: file.name.clone(),
                                why: "missing",
                            });
                            continue;
                        }
                        Err(_) => None,
                    },
                    (None, Some(sha)) if blobs.has(sha).unwrap_or(false) => {
                        Some((sha.clone(), file.size.unwrap_or(0)))
                    }
                    _ => None,
                };
                let Some((sha256, size)) = stored else {
                    left_out.push(LeftOut {
                        note: note.title.clone(),
                        name: file.name.clone(),
                        why: "unreadable",
                    });
                    continue;
                };
                note.files.push(ImportFile {
                    name: file.name,
                    mime: file.mime,
                    size: i64::try_from(size).unwrap_or(i64::MAX),
                    sha256,
                });
            }
            notes.push(note);
        }
        let place = open
            .place
            .as_mut()
            .ok_or_else(|| CommandError::new("invalid", "the import has no folder yet"))?;
        let batch = ImportBatch {
            importer: batch.importer,
            folders: batch.folders,
            notes,
        };
        let notes = SESSION.with_borrow_mut(|session| {
            let session = session.as_mut().ok_or_else(CommandError::locked)?;
            Ok::<_, CommandError>(session.store.import_batch(place, &batch)?)
        })?;
        answer(Written { notes, left_out })
    })
}

/// The commands that take json and maybe bytes, `None` for another name.
pub fn run(name: &str, a: &Args, bytes: Option<Vec<u8>>) -> Option<CommandResult<Value>> {
    Some(match name {
        "last_report" => Ok(REPORT.with_borrow_mut(std::mem::take)),
        "backup_default_name" => answer(
            app_core::backup::default_name(app_core::clock::Clock::now(
                &app_core::clock::SystemClock,
            ))
            .to_string_lossy()
            .into_owned(),
        ),
        "backup_encrypted" => bytes.ok_or_else(no_file).and_then(|bytes| {
            answer(app_core::backup::is_encrypted_file(&mut Cursor::new(
                bytes,
            ))?)
        }),
        "backup_inspect" => backup(bytes, a).and_then(|mut backup| {
            let data = backup.data()?;
            let ids: Vec<_> = data.notes.iter().map(|n| n.id).collect();
            let present = SESSION.with_borrow(|session| {
                let session = session.as_ref().ok_or_else(CommandError::locked)?;
                Ok::<_, CommandError>(session.store.notes_present(&ids)?.len())
            })?;
            answer(Inspected {
                manifest: backup.manifest().clone(),
                present,
            })
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
            let root: FolderId = with(|s| s.store.create_folder(None, &name))
                .and_then(|folder| {
                    serde_json::from_value::<app_core::model::Folder>(folder)
                        .map_err(|e| CommandError::new("invalid", e.to_string()))
                })?
                .id;
            import(id, |open| {
                open.place = Some(ImportPlace::new(root));
                Ok(())
            })?;
            answer(root)
        })(),
        "import_put" => bytes.ok_or_else(no_file).and_then(|bytes| {
            with(|s| {
                Ok(Stored {
                    sha256: s.blobs.put(&bytes)?,
                    size: bytes.len() as u64,
                })
            })
        }),
        "import_write" => import_write(a),
        "import_close" => a.get::<u32>("id").map(|id| {
            IMPORTS.with_borrow_mut(|(_, open)| open.remove(&id));
            Value::Null
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
            let summary = ExportSummary {
                notes: report.notes,
                folders: report.folders,
                attachments: report.attachments,
                missing_attachments: report.missing_attachments,
            };
            REPORT.with_borrow_mut(|report| *report = answer(summary).unwrap_or_default());
            Ok(out)
        })(),
        "import_read" => (|| {
            let (id, path): (u32, String) = (a.get("id")?, a.get("path")?);
            import(id, |open| Ok(open.source.read(&path)?))
        })(),
        _ => return None,
    })
}
