// SPDX-License-Identifier: AGPL-3.0-or-later

//! Writing what an import found: folders and notes with their own dates,
//! pin, trash and tags, and their files, a batch per transaction. Each note
//! remembers where it came from, so importing the same export again skips
//! it.

use std::collections::{HashMap, HashSet};

use rusqlite::{OptionalExtension, params};
use serde::Deserialize;

use super::{Store, attachments::free_name, clean_text, folders, reindex, tags};
use crate::error::{Error, Result};
use crate::id::{AttachmentId, FolderId, NoteId};

/// A folder of the export.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFolder {
    /// Its key in the export, which notes and other folders refer to.
    pub key: String,
    /// The folder it is in, `None` for the top of the import.
    pub parent: Option<String>,
    /// Name.
    pub name: String,
}

/// A file of a note, already in the vault's files.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFile {
    /// Its name, which the body refers to as `attachments/<name>`.
    pub name: String,
    /// Media type.
    pub mime: String,
    /// Size in bytes.
    pub size: i64,
    /// Hex SHA-256 of the content.
    pub sha256: String,
}

/// A note of the export.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportNote {
    /// What identifies it in the export, the same each time it is exported.
    pub key: String,
    /// Its folder's key, `None` for the top of the import.
    pub folder: Option<String>,
    /// Title.
    pub title: String,
    /// Markdown body.
    pub body: String,
    /// Tag names.
    pub tags: Vec<String>,
    /// Created, unix milliseconds, when the export says.
    pub created: Option<i64>,
    /// Last changed, unix milliseconds, when the export says.
    pub updated: Option<i64>,
    /// Pinned.
    pub pinned: bool,
    /// In the other app's trash: it goes to the trash here.
    pub trashed: bool,
    /// Its files.
    pub files: Vec<ImportFile>,
}

/// One batch of an import.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportBatch {
    /// Which importer read it (`notion`, `joplin`...).
    pub importer: String,
    /// Folders, parents before their children.
    pub folders: Vec<ImportFolder>,
    /// Notes.
    pub notes: Vec<ImportNote>,
}

/// Where an import goes: the folder made for it, and the folders made so
/// far by key. Kept by the caller from one batch to the next.
#[derive(Debug, Clone)]
pub struct ImportPlace {
    /// The import's own folder.
    pub root: FolderId,
    /// Folders of the export already made.
    pub folders: HashMap<String, FolderId>,
}

impl ImportPlace {
    /// An import into `root`.
    pub fn new(root: FolderId) -> Self {
        Self {
            root,
            folders: HashMap::new(),
        }
    }

    /// The folder made for a key, in this batch (`made`, not kept until
    /// the batch is) or an earlier one, else the import's own.
    fn folder(&self, made: &HashMap<String, FolderId>, key: Option<&String>) -> FolderId {
        key.and_then(|k| made.get(k).or_else(|| self.folders.get(k)))
            .copied()
            .unwrap_or(self.root)
    }
}

impl Store {
    /// The keys among `keys` already imported by `importer` whose note is
    /// still in the vault (in the trash too).
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn imported(&self, importer: &str, keys: &[String]) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT 1 FROM imports i JOIN notes n ON n.id = i.note_id
             WHERE i.importer = ?1 AND i.source_key = ?2",
        )?;
        let mut found = HashSet::new();
        for key in keys {
            if stmt.exists(params![importer, key])? {
                found.insert(key.clone());
            }
        }
        Ok(found)
    }

    /// Writes one batch of an import in one transaction, with its sync
    /// record. Notes keep the dates of the export (now when it has none),
    /// notes it had in the trash go to the trash from now, so they stay the
    /// usual time.
    ///
    /// # Errors
    ///
    /// The import's folder is missing or in the trash, a file is malformed,
    /// database errors. Nothing of the batch is written then.
    pub fn import_batch(
        &mut self,
        place: &mut ImportPlace,
        batch: &ImportBatch,
    ) -> Result<Vec<(String, NoteId)>> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        folders::require_live(&tx, place.root)?;
        let space = folders::space_of(&tx, place.root)?;
        let mut made = HashMap::new();
        for folder in &batch.folders {
            if place.folders.contains_key(&folder.key) || made.contains_key(&folder.key) {
                continue;
            }
            let parent = place.folder(&made, folder.parent.as_ref());
            let name = folders::clean_name(&folder.name).unwrap_or_else(|_| "Folder".to_owned());
            let id = FolderId::new();
            tx.execute(
                "INSERT INTO folders (id, parent_id, name, position, created_at, updated_at, space_id)
                 VALUES (?1, ?2, ?3,
                         (SELECT coalesce(max(position) + 1, 0) FROM folders WHERE parent_id IS ?2),
                         ?4, ?4, ?5)",
                params![id, parent, name, now, space],
            )?;
            made.insert(folder.key.clone(), id);
        }
        let mut written = Vec::with_capacity(batch.notes.len());
        for note in &batch.notes {
            let id = NoteId::new();
            let folder = place.folder(&made, note.folder.as_ref());
            let created = note.created.or(note.updated).unwrap_or(now);
            let updated = note.updated.unwrap_or(created).max(created);
            tx.execute(
                "INSERT INTO notes (id, folder_id, title, body, pinned, created_at, updated_at,
                                    trashed_at, space_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    folder,
                    clean_text(&note.title),
                    clean_text(&note.body),
                    note.pinned,
                    created,
                    updated,
                    note.trashed.then_some(now),
                    space
                ],
            )?;
            tags::replace_note_tags(&tx, id, &note.tags, now)?;
            let mut taken = HashSet::new();
            for file in &note.files {
                let name = clean_text(&file.name).trim().to_owned();
                if name.is_empty()
                    || file.size < 0
                    || file.sha256.len() != 64
                    || !file
                        .sha256
                        .bytes()
                        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                {
                    return Err(Error::Invalid(format!(
                        "the file {:?} of {:?} is malformed",
                        file.name, note.title
                    )));
                }
                let name = free_name(&name, &taken);
                taken.insert(name.clone());
                tx.execute(
                    "INSERT INTO attachments (id, note_id, file_name, mime_type, size, sha256, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        AttachmentId::new(),
                        id,
                        name,
                        file.mime.trim(),
                        file.size,
                        file.sha256,
                        created
                    ],
                )?;
            }
            reindex(&tx, id)?;
            if !note.key.is_empty() {
                tx.execute(
                    "INSERT OR REPLACE INTO imports (importer, source_key, note_id, at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![batch.importer, note.key, id, now],
                )?;
            }
            written.push((note.key.clone(), id));
        }
        crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        place.folders.extend(made);
        Ok(written)
    }

    /// The note an earlier import wrote for a key, if still there.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn imported_note(&self, importer: &str, key: &str) -> Result<Option<NoteId>> {
        Ok(self
            .conn
            .query_row(
                "SELECT i.note_id FROM imports i JOIN notes n ON n.id = i.note_id
                 WHERE i.importer = ?1 AND i.source_key = ?2",
                params![importer, key],
                |row| row.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::store;
    use super::*;
    use crate::clock::Clock as _;
    use crate::model::{NoteScope, NoteSort};

    fn note(key: &str, folder: Option<&str>, title: &str) -> ImportNote {
        ImportNote {
            key: key.into(),
            folder: folder.map(Into::into),
            title: title.into(),
            body: format!("about {title}"),
            ..ImportNote::default()
        }
    }

    #[test]
    fn a_batch_keeps_tree_dates_tags_pin_and_trash() {
        let (mut store, clock) = store();
        let root = store.create_folder(None, "Notion, 27 Oct 2026").unwrap().id;
        let mut place = ImportPlace::new(root);
        let mut old = note("n1", Some("f2"), "Old one");
        old.created = Some(1_500_000_000_000);
        old.updated = Some(1_600_000_000_000);
        old.tags = vec!["work".into(), "Work".into()];
        old.pinned = true;
        let mut gone = note("n2", None, "Gone");
        gone.trashed = true;
        let mut late = note("n3", Some("unknown"), "Late");
        late.updated = Some(1_000);
        let batch = ImportBatch {
            importer: "notion".into(),
            folders: vec![
                ImportFolder {
                    key: "f1".into(),
                    parent: None,
                    name: "Projects".into(),
                },
                ImportFolder {
                    key: "f2".into(),
                    parent: Some("f1".into()),
                    name: " \n".into(),
                },
            ],
            notes: vec![old, gone, late],
        };
        let written = store.import_batch(&mut place, &batch).unwrap();
        assert_eq!(written.len(), 3);
        assert_eq!(place.folders.len(), 2);

        let folders = store.list_folders().unwrap();
        let projects = folders.iter().find(|f| f.name == "Projects").unwrap();
        assert_eq!(projects.parent_id, Some(root));
        let inner = folders.iter().find(|f| f.name == "Folder").unwrap();
        assert_eq!(inner.parent_id, Some(projects.id));

        let old = store.note(written[0].1).unwrap();
        assert_eq!(old.folder_id, Some(inner.id));
        assert_eq!(
            (old.created_at, old.updated_at),
            (1_500_000_000_000, 1_600_000_000_000)
        );
        assert_eq!(old.tags, ["work"]);
        assert!(old.pinned);
        let gone = store.note(written[1].1).unwrap();
        assert_eq!(gone.folder_id, Some(root));
        assert_eq!(gone.trashed_at, Some(clock.now()));
        // only an updated date: it is the created one too
        let late = store.note(written[2].1).unwrap();
        assert_eq!(late.folder_id, Some(root));
        assert_eq!((late.created_at, late.updated_at), (1_000, 1_000));
        // search and links see them
        let found = store
            .list_notes(NoteScope::Folder(Some(inner.id)), NoteSort::default())
            .unwrap();
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn later_batches_find_earlier_folders_and_the_same_export_is_skipped() {
        let (mut store, _) = store();
        let root = store.create_folder(None, "Import").unwrap().id;
        let mut place = ImportPlace::new(root);
        let first = ImportBatch {
            importer: "joplin".into(),
            folders: vec![ImportFolder {
                key: "a".into(),
                parent: None,
                name: "A".into(),
            }],
            notes: vec![note("1", Some("a"), "One")],
        };
        store.import_batch(&mut place, &first).unwrap();
        let second = ImportBatch {
            importer: "joplin".into(),
            folders: vec![],
            notes: vec![note("2", Some("a"), "Two")],
        };
        let written = store.import_batch(&mut place, &second).unwrap();
        let two = store.note(written[0].1).unwrap();
        assert_eq!(two.folder_id, place.folders.get("a").copied());

        let keys: Vec<String> = ["1", "2", "3"].map(Into::into).to_vec();
        let known = store.imported("joplin", &keys).unwrap();
        assert_eq!(known, HashSet::from(["1".to_owned(), "2".to_owned()]));
        assert!(store.imported("notion", &keys).unwrap().is_empty());
        // a note deleted for good is imported again next time
        store.trash_note(written[0].1).unwrap();
        store.purge_note(written[0].1).unwrap();
        assert_eq!(store.imported("joplin", &keys).unwrap().len(), 1);
        assert!(store.imported_note("joplin", "2").unwrap().is_none());
    }

    #[test]
    fn files_get_free_names_and_a_bad_one_writes_nothing() {
        let (mut store, _) = store();
        let root = store.create_folder(None, "Import").unwrap().id;
        let mut place = ImportPlace::new(root);
        let sha = crate::blobs::sha256_hex(b"png");
        let file = |name: &str| ImportFile {
            name: name.into(),
            mime: "image/png".into(),
            size: 3,
            sha256: sha.clone(),
        };
        let mut with_files = note("1", None, "Pictures");
        with_files.files = vec![file("a.png"), file("a.png")];
        let written = store
            .import_batch(
                &mut place,
                &ImportBatch {
                    importer: "md".into(),
                    folders: vec![],
                    notes: vec![with_files],
                },
            )
            .unwrap();
        let mut names: Vec<_> = store
            .list_attachments(written[0].1)
            .unwrap()
            .into_iter()
            .map(|a| a.file_name)
            .collect();
        names.sort();
        assert_eq!(names.len(), 2);
        assert_ne!(names[0], names[1]);

        let mut bad = note("2", None, "Bad");
        bad.files = vec![ImportFile {
            sha256: "nope".into(),
            ..file("b.png")
        }];
        let before = store.counts().unwrap();
        let batch = ImportBatch {
            importer: "md".into(),
            folders: vec![ImportFolder {
                key: "x".into(),
                parent: None,
                name: "X".into(),
            }],
            notes: vec![note("3", None, "Fine"), bad],
        };
        assert!(store.import_batch(&mut place, &batch).is_err());
        assert_eq!(store.counts().unwrap(), before);
        assert!(place.folders.is_empty(), "nothing kept of a failed batch");
    }

    #[test]
    fn an_import_into_a_trashed_folder_is_refused() {
        let (mut store, _) = store();
        let root = store.create_folder(None, "Import").unwrap().id;
        store.trash_folder(root).unwrap();
        let mut place = ImportPlace::new(root);
        let batch = ImportBatch {
            importer: "md".into(),
            folders: vec![],
            notes: vec![note("1", None, "x")],
        };
        assert!(store.import_batch(&mut place, &batch).is_err());
    }
}
