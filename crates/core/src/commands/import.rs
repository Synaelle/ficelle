// SPDX-License-Identifier: AGPL-3.0-or-later

//! Importing from other apps. The page reads the formats; the shell opens
//! the export, hands its entries to the page as asked, stores the files
//! straight from the export and writes the notes a batch at a time.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::blobs::BlobStore;
use crate::error::{Error, Result};
use crate::id::{FolderId, NoteId};
use crate::import::source::{Entry, Source};
use crate::store::{ImportBatch, ImportFile, ImportFolder, ImportNote, ImportPlace};

/// An export being imported.
struct Open {
    source: Source,
    place: Option<ImportPlace>,
}

/// The exports open, by the number the page knows them by.
#[derive(Default)]
pub struct Imports {
    open: HashMap<u32, Open>,
    next: u32,
}

/// An export just opened, as the page reads it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    id: u32,
    /// The export's file or folder name.
    name: String,
    entries: Vec<Entry>,
    /// Its directories, empty ones included.
    dirs: Vec<String>,
    /// What the export holds that could not be taken.
    left_out: Vec<String>,
}

/// Bytes the page decoded (an Evernote picture, a data url), stored in the
/// vault's files without being attached yet: the batch names them by hash.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    /// Hex SHA-256 of the content.
    pub sha256: String,
    /// Size in bytes.
    pub size: u64,
}

/// A file of a note as the page names it: an entry of the export, or bytes
/// it stored before ([`Stored`]).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageFile {
    name: String,
    mime: String,
    entry: Option<String>,
    sha256: Option<String>,
    size: Option<u64>,
}

/// A note of a batch, with its files as the page names them.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageNote {
    #[serde(flatten)]
    note: ImportNote,
    #[serde(default)]
    sources: Vec<PageFile>,
}

/// A batch of notes as the page sends it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageBatch {
    importer: String,
    #[serde(default)]
    folders: Vec<ImportFolder>,
    notes: Vec<PageNote>,
}

/// A file left out, for the report.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOut {
    note: String,
    name: String,
    /// `missing`, `tooLarge`, `typeRefused`, `tooManyFiles` or `unreadable`.
    why: &'static str,
}

/// A batch written: its notes by key, and the files left out.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    notes: Vec<(String, NoteId)>,
    left_out: Vec<LeftOut>,
}

fn closed() -> Error {
    Error::NotFound("this import")
}

impl Imports {
    fn get(&mut self, id: u32) -> Result<&mut Open> {
        self.open.get_mut(&id).ok_or_else(closed)
    }

    /// Keeps an export opened from the file or folder `name`.
    pub fn open(&mut self, source: Source, name: String) -> Opened {
        let opened = Opened {
            id: self.next,
            name,
            entries: source.entries().to_vec(),
            dirs: source.dirs().to_vec(),
            left_out: source.left_out().to_vec(),
        };
        self.next += 1;
        self.open.insert(
            opened.id,
            Open {
                source,
                place: None,
            },
        );
        opened
    }

    /// One entry of an open export, whole.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] once the import is closed, as [`Source::read`].
    pub fn read(&mut self, id: u32, path: &str) -> Result<Vec<u8>> {
        self.get(id)?.source.read(path)
    }

    /// Starts the import into its own folder, which `make_root` makes:
    /// everything of this import goes in it.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] once the import is closed, else `make_root`'s.
    pub fn begin<E: From<Error>>(
        &mut self,
        id: u32,
        make_root: impl FnOnce() -> std::result::Result<FolderId, E>,
    ) -> std::result::Result<FolderId, E> {
        let open = self.get(id)?;
        let root = make_root()?;
        open.place = Some(ImportPlace::new(root));
        Ok(root)
    }

    /// Writes a batch: its files stored from the export by `store` (which
    /// returns their hash and size), then its folders and notes by `write`.
    /// With a server's `policy`, the files it refuses are left out.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] once the import is closed, [`Error::Invalid`]
    /// before it began, else `write`'s.
    pub fn write<E: From<Error>>(
        &mut self,
        id: u32,
        batch: PageBatch,
        blobs: &BlobStore,
        policy: Option<&app_protocol::files::FilesPolicy>,
        mut store: impl FnMut(&mut Source, &str) -> Result<(String, u64)>,
        write: impl FnOnce(
            &mut ImportPlace,
            &ImportBatch,
        ) -> std::result::Result<Vec<(String, NoteId)>, E>,
    ) -> std::result::Result<Written, E> {
        let open = self.get(id)?;
        let mut left_out = Vec::new();
        let mut notes = Vec::with_capacity(batch.notes.len());
        for PageNote { mut note, sources } in batch.notes {
            for file in sources {
                let mut leave = |why| {
                    left_out.push(LeftOut {
                        note: note.title.clone(),
                        name: file.name.clone(),
                        why,
                    });
                };
                if let Some(p) = policy {
                    if p.max_files_per_note > 0
                        && note.files.len() as u64 >= u64::from(p.max_files_per_note)
                    {
                        leave("tooManyFiles");
                        continue;
                    }
                    if !app_protocol::files::allows(p, &file.name, &file.mime) {
                        leave("typeRefused");
                        continue;
                    }
                }
                let stored = match (&file.entry, &file.sha256) {
                    (Some(entry), _) => match store(&mut open.source, entry) {
                        Ok(stored) => Some(stored),
                        Err(Error::NotFound(_)) => {
                            leave("missing");
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
                    leave("unreadable");
                    continue;
                };
                let limit = policy.map_or(0, |p| p.max_file_bytes);
                if limit > 0 && size > limit {
                    leave("tooLarge");
                    continue;
                }
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
            .ok_or_else(|| Error::Invalid("the import has no folder yet".into()))?;
        let batch = ImportBatch {
            importer: batch.importer,
            folders: batch.folders,
            notes,
        };
        let notes = write(place, &batch)?;
        Ok(Written { notes, left_out })
    }

    /// Closes an export, removing what opening it wrote to the temporary
    /// folder. Returns the folder of its import, to end, if it began.
    pub fn close(&mut self, id: u32) -> Option<FolderId> {
        self.open.remove(&id)?.place.map(|place| place.root)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use app_protocol::files::FilesPolicy;

    use super::*;
    use crate::vault::{Encryption, Vault};

    /// A tar export of `files`, by path.
    fn export(files: &[(&str, &[u8])]) -> Source {
        let mut tar = tar::Builder::new(Vec::new());
        for (path, data) in files {
            let mut header = tar::Header::new_ustar();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, path, *data).unwrap();
        }
        let bytes = tar.into_inner().unwrap();
        Source::open_file(Cursor::new(bytes), "export.tar").unwrap()
    }

    fn file(name: &str, mime: &str, entry: &str) -> serde_json::Value {
        serde_json::json!({ "name": name, "mime": mime, "entry": entry })
    }

    #[test]
    fn a_batch_leaves_out_what_the_files_policy_refuses_and_writes_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, mut store) = Vault::create(dir.path(), Encryption::Off).unwrap();
        let blobs = vault.blobs(&mut store).unwrap();
        let mut imports = Imports::default();
        let opened = imports.open(
            export(&[("plan.jpg", b"jpeg"), ("setup.exe", b"MZ")]),
            "export.tar".into(),
        );
        let begun = imports.begin(opened.id, || store.begin_import("Garden"));
        assert!(begun.is_ok());
        let batch: PageBatch = serde_json::from_value(serde_json::json!({
            "importer": "markdown",
            "notes": [{
                "key": "trellis",
                "title": "Tomato trellis plan",
                "body": "![](attachments/plan.jpg)",
                "sources": [
                    file("plan.jpg", "image/jpeg", "plan.jpg"),
                    file("setup.exe", "application/x-msdownload", "setup.exe"),
                    file("lost.png", "image/png", "lost.png"),
                ],
            }],
        }))
        .unwrap();
        let policy = FilesPolicy {
            max_file_bytes: 0,
            quota_bytes: 0,
            allowed_types: Vec::new(),
            blocked_types: vec![".exe".into()],
            max_files_per_note: 0,
            image_max_px: 0,
            image_quality: 80,
            thumbnails: false,
            retention_days: 30,
        };
        let written = imports
            .write(
                opened.id,
                batch,
                &blobs,
                Some(&policy),
                |source, entry| source.store(entry, &blobs),
                |place, batch| store.import_batch(place, batch),
            )
            .unwrap();
        let why: Vec<_> = written
            .left_out
            .iter()
            .map(|l| (l.name.as_str(), l.why))
            .collect();
        assert_eq!(why, [("setup.exe", "typeRefused"), ("lost.png", "missing")]);
        let (_, id) = &written.notes[0];
        let files = store.list_attachments(*id).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "plan.jpg");
        assert!(imports.close(opened.id).is_some());
        assert!(matches!(
            imports.read(opened.id, "plan.jpg"),
            Err(Error::NotFound(_))
        ));
    }
}
