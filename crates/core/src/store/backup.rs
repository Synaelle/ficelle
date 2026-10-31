// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a backup holds of the store, and putting it back. Ids are kept, so
//! links, versions and files come back as they were. A note or folder
//! already in the vault is left as it is: restoring twice changes nothing.

use std::collections::{HashMap, HashSet};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Store, folders, reindex, tags};
use crate::error::Result;
use crate::id::{FolderId, NoteId};

/// A folder in a backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFolder {
    /// Its id, kept.
    pub id: FolderId,
    /// The folder it is in, `None` at the top.
    pub parent_id: Option<FolderId>,
    /// Name.
    pub name: String,
    /// Order among its siblings.
    pub position: i64,
    /// Created, unix milliseconds.
    pub created_at: i64,
    /// Last changed, unix milliseconds.
    pub updated_at: i64,
    /// In the trash since then, unix milliseconds.
    pub trashed_at: Option<i64>,
}

/// A note in a backup, with its tag names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupNote {
    /// Its id, kept.
    pub id: NoteId,
    /// Its folder, `None` at the top.
    pub folder_id: Option<FolderId>,
    /// Title.
    pub title: String,
    /// Markdown body.
    pub body: String,
    /// Pinned.
    pub pinned: bool,
    /// Tag names.
    pub tags: Vec<String>,
    /// Created, unix milliseconds.
    pub created_at: i64,
    /// Last changed, unix milliseconds.
    pub updated_at: i64,
    /// In the trash since then, unix milliseconds.
    pub trashed_at: Option<i64>,
}

/// A version from a note's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupVersion {
    /// Its id, kept.
    pub id: String,
    /// The note it belongs to.
    pub note_id: NoteId,
    /// Title.
    pub title: String,
    /// Markdown body.
    pub body: String,
    /// Created, unix milliseconds.
    pub created_at: i64,
    /// Why it was kept (`edit`, `restore`...).
    pub reason: String,
}

/// A note's file, its content under `files/<sha256>` in the backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupAttachment {
    /// Its id, kept.
    pub id: String,
    /// The note it belongs to.
    pub note_id: NoteId,
    /// File name.
    pub file_name: String,
    /// Media type.
    pub mime_type: String,
    /// Size in bytes.
    pub size: i64,
    /// Hex SHA-256 of the content.
    pub sha256: String,
    /// Created, unix milliseconds.
    pub created_at: i64,
    /// Its preview, when it has one.
    pub thumb_sha256: Option<String>,
}

/// Everything a backup holds but the files' content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupData {
    /// Folders.
    pub folders: Vec<BackupFolder>,
    /// Notes.
    pub notes: Vec<BackupNote>,
    /// Versions of notes.
    pub versions: Vec<BackupVersion>,
    /// Files of notes.
    pub attachments: Vec<BackupAttachment>,
}

impl BackupData {
    /// Hashes of the files the backup's notes use, previews too.
    pub fn file_hashes(&self) -> HashSet<String> {
        self.attachments
            .iter()
            .flat_map(|a| std::iter::once(a.sha256.clone()).chain(a.thumb_sha256.clone()))
            .collect()
    }
}

/// What a restore did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    /// Notes.
    pub notes: usize,
    /// Folders.
    pub folders: usize,
    /// Versions of notes.
    pub versions: usize,
    /// Files.
    pub files: usize,
    /// Notes already in the vault, left as they are.
    pub skipped: usize,
    /// Files whose content was not in the backup (never fetched on the
    /// device that made it): their notes come back without them.
    pub files_missing: usize,
}

/// Notes per transaction when restoring.
const BATCH: usize = 500;

impl Store {
    /// Everything of the vault a backup keeps: folders, notes with their
    /// tags, history, and the descriptions of the files in use.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn backup_data(&self) -> Result<BackupData> {
        let mut data = BackupData::default();
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_id, name, position, created_at, updated_at, trashed_at
             FROM folders ORDER BY position, id",
        )?;
        data.folders = stmt
            .query_map([], |r| {
                Ok(BackupFolder {
                    id: r.get(0)?,
                    parent_id: r.get(1)?,
                    name: r.get(2)?,
                    position: r.get(3)?,
                    created_at: r.get(4)?,
                    updated_at: r.get(5)?,
                    trashed_at: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT id, folder_id, title, body, pinned, created_at, updated_at, trashed_at
             FROM notes ORDER BY created_at, id",
        )?;
        let notes: Vec<BackupNote> = stmt
            .query_map([], |r| {
                Ok(BackupNote {
                    id: r.get(0)?,
                    folder_id: r.get(1)?,
                    title: r.get(2)?,
                    body: r.get(3)?,
                    pinned: r.get(4)?,
                    tags: Vec::new(),
                    created_at: r.get(5)?,
                    updated_at: r.get(6)?,
                    trashed_at: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut tags_of: HashMap<NoteId, Vec<String>> = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT nt.note_id, t.name FROM note_tags nt JOIN tags t ON t.id = nt.tag_id
             ORDER BY t.name",
        )?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, NoteId>(0)?, r.get::<_, String>(1)?)))? {
            let (note, tag) = row?;
            tags_of.entry(note).or_default().push(tag);
        }
        data.notes = notes
            .into_iter()
            .map(|mut n| {
                n.tags = tags_of.remove(&n.id).unwrap_or_default();
                n
            })
            .collect();
        let mut stmt = self.conn.prepare(
            "SELECT id, note_id, title, body, created_at, reason FROM note_versions
             ORDER BY created_at, id",
        )?;
        data.versions = stmt
            .query_map([], |r| {
                Ok(BackupVersion {
                    id: r.get(0)?,
                    note_id: r.get(1)?,
                    title: r.get(2)?,
                    body: r.get(3)?,
                    created_at: r.get(4)?,
                    reason: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        // removed files are on their way out, a backup leaves them
        let mut stmt = self.conn.prepare(
            "SELECT id, note_id, file_name, mime_type, size, sha256, created_at, thumb_sha256
             FROM attachments WHERE removed_at IS NULL ORDER BY created_at, id",
        )?;
        data.attachments = stmt
            .query_map([], |r| {
                Ok(BackupAttachment {
                    id: r.get(0)?,
                    note_id: r.get(1)?,
                    file_name: r.get(2)?,
                    mime_type: r.get(3)?,
                    size: r.get(4)?,
                    sha256: r.get(5)?,
                    created_at: r.get(6)?,
                    thumb_sha256: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(data)
    }

    /// The notes of a backup already in the vault (in the trash too).
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn notes_present(&self, ids: &[NoteId]) -> Result<HashSet<NoteId>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT 1 FROM notes WHERE id = ?1")?;
        let mut found = HashSet::new();
        for id in ids {
            if stmt.exists([id])? {
                found.insert(*id);
            }
        }
        Ok(found)
    }

    /// Puts a backup back with its ids. Folders and notes already here are
    /// left as they are, a restored folder goes under its parent when the
    /// vault has it, else at the top, a note in its folder when there is
    /// one. A file is attached only when `has_file` says its content is
    /// in the vault's files.
    ///
    /// # Errors
    ///
    /// Database errors. Batches written before stay.
    pub fn restore_backup(
        &mut self,
        data: &BackupData,
        has_file: &dyn Fn(&str) -> bool,
    ) -> Result<RestoreReport> {
        let mut report = RestoreReport {
            folders: self.restore_folders(&data.folders)?,
            ..RestoreReport::default()
        };
        let now = self.now();

        let present = self.notes_present(&data.notes.iter().map(|n| n.id).collect::<Vec<_>>())?;
        report.skipped = present.len();
        let restored: Vec<&BackupNote> = data
            .notes
            .iter()
            .filter(|n| !present.contains(&n.id))
            .collect();
        let mut versions: HashMap<NoteId, Vec<&BackupVersion>> = HashMap::new();
        for v in &data.versions {
            versions.entry(v.note_id).or_default().push(v);
        }
        let mut files: HashMap<NoteId, Vec<&BackupAttachment>> = HashMap::new();
        for a in &data.attachments {
            files.entry(a.note_id).or_default().push(a);
        }
        for batch in restored.chunks(BATCH) {
            let tx = self.conn.transaction()?;
            for note in batch {
                let folder = match note.folder_id {
                    Some(f) => tx
                        .query_row("SELECT 1 FROM folders WHERE id = ?1", [f], |_| Ok(()))
                        .optional()?
                        .map(|()| f),
                    None => None,
                };
                let space = match folder {
                    Some(f) => folders::space_of(&tx, f)?,
                    None => String::new(),
                };
                tx.execute(
                    "INSERT INTO notes (id, folder_id, title, body, pinned, created_at, updated_at,
                                        trashed_at, space_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        note.id,
                        folder,
                        note.title,
                        note.body,
                        note.pinned,
                        note.created_at,
                        note.updated_at,
                        note.trashed_at,
                        space
                    ],
                )?;
                tags::replace_note_tags(&tx, note.id, &note.tags, now)?;
                for v in versions.get(&note.id).into_iter().flatten() {
                    report.versions += tx.execute(
                        "INSERT OR IGNORE INTO note_versions (id, note_id, title, body, created_at, reason)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![v.id, v.note_id, v.title, v.body, v.created_at, v.reason],
                    )?;
                }
                for a in files.get(&note.id).into_iter().flatten() {
                    if !has_file(&a.sha256) {
                        report.files_missing += 1;
                        continue;
                    }
                    let thumb = a.thumb_sha256.as_deref().filter(|t| has_file(t));
                    report.files += tx.execute(
                        "INSERT OR IGNORE INTO attachments
                             (id, note_id, file_name, mime_type, size, sha256, created_at, thumb_sha256)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            a.id,
                            a.note_id,
                            a.file_name,
                            a.mime_type,
                            a.size,
                            a.sha256,
                            a.created_at,
                            thumb
                        ],
                    )?;
                }
                reindex(&tx, note.id)?;
                report.notes += 1;
            }
            crate::sync::finish(tx, &mut self.docs, self.clock.now())?;
        }
        Ok(report)
    }

    /// The folders of a backup missing here, parents first, in one
    /// transaction. A folder whose parent is neither here nor in the backup
    /// goes at the top, so do folders in a loop of parents.
    fn restore_folders(&mut self, all: &[BackupFolder]) -> Result<usize> {
        let ids: HashSet<FolderId> = all.iter().map(|f| f.id).collect();
        let mut ordered: Vec<BackupFolder> = Vec::with_capacity(all.len());
        let mut placed: HashSet<FolderId> = HashSet::new();
        let mut left: Vec<&BackupFolder> = all.iter().collect();
        while !left.is_empty() {
            let (ready, waiting): (Vec<&BackupFolder>, Vec<&BackupFolder>) =
                left.into_iter().partition(|f| {
                    f.parent_id
                        .is_none_or(|p| placed.contains(&p) || !ids.contains(&p))
                });
            if ready.is_empty() {
                ordered.extend(waiting.into_iter().map(|f| BackupFolder {
                    parent_id: None,
                    ..f.clone()
                }));
                break;
            }
            for folder in ready {
                placed.insert(folder.id);
                ordered.push(folder.clone());
            }
            left = waiting;
        }

        let now = self.now();
        let tx = self.conn.transaction()?;
        let mut made = 0;
        for folder in &ordered {
            let exists = |id: FolderId| -> Result<bool> {
                Ok(tx
                    .query_row("SELECT 1 FROM folders WHERE id = ?1", [id], |_| Ok(()))
                    .optional()?
                    .is_some())
            };
            if exists(folder.id)? {
                continue;
            }
            let parent = match folder.parent_id {
                Some(p) if exists(p)? => Some(p),
                _ => None,
            };
            let space = match parent {
                Some(p) => folders::space_of(&tx, p)?,
                None => String::new(),
            };
            tx.execute(
                "INSERT INTO folders (id, parent_id, name, position, created_at, updated_at,
                                      trashed_at, space_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    folder.id,
                    parent,
                    folder.name,
                    folder.position,
                    folder.created_at,
                    folder.updated_at,
                    folder.trashed_at,
                    space
                ],
            )?;
            made += 1;
        }
        crate::sync::finish(tx, &mut self.docs, now)?;
        Ok(made)
    }
}
