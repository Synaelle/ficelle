// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment descriptions, which note uses which file. The files are kept
//! by content hash in [`crate::blobs`]. Descriptions sync with the note,
//! the files go to the server separately: this module also keeps track of
//! which files the server holds.

use std::collections::HashSet;

use rusqlite::{Connection, Row, params};
use zeroize::Zeroizing;

use super::{Store, clean_text};
use crate::blobs::is_sha256_hex;
use crate::clock::Timestamp;
use crate::crypto::random_bytes;
use crate::error::{Error, Result};
use crate::id::{AttachmentId, NoteId};
use crate::model::{Attachment, NoteAttachment};

const COLUMNS: &str = "id, note_id, file_name, mime_type, size, sha256, created_at, thumb_sha256";

fn attachment_from_row(row: &Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get(0)?,
        note_id: row.get(1)?,
        file_name: row.get(2)?,
        mime_type: row.get(3)?,
        size: row.get(4)?,
        sha256: row.get(5)?,
        created_at: row.get(6)?,
        thumb_sha256: row.get(7)?,
    })
}

/// An attachment row as the sync documents see it, removed ones included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileRow {
    pub id: AttachmentId,
    pub note_id: NoteId,
    pub file_name: String,
    pub mime_type: String,
    pub size: i64,
    pub sha256: String,
    pub created_at: Timestamp,
    pub thumb_sha256: Option<String>,
    pub epoch: Option<i64>,
    pub removed_at: Option<Timestamp>,
}

/// Every attachment row of a note, removed ones included, by id.
pub(crate) fn file_rows(conn: &Connection, note: NoteId) -> Result<Vec<FileRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS}, epoch, removed_at FROM attachments WHERE note_id = ?1 ORDER BY id"
    ))?;
    let rows = stmt
        .query_map([note], |row| {
            Ok(FileRow {
                id: row.get(0)?,
                note_id: row.get(1)?,
                file_name: row.get(2)?,
                mime_type: row.get(3)?,
                size: row.get(4)?,
                sha256: row.get(5)?,
                created_at: row.get(6)?,
                thumb_sha256: row.get(7)?,
                epoch: row.get(8)?,
                removed_at: row.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Puts the given rows in place of the rows with the same ids. They go
/// first, so names can move between them without clashing. A live file
/// arriving this way (attached elsewhere, or back with a note from the
/// trash) may have been let go on the server meanwhile: the next round asks
/// again.
pub(crate) fn replace_file_rows(conn: &Connection, note: NoteId, rows: &[FileRow]) -> Result<()> {
    for row in rows.iter().filter(|r| r.removed_at.is_none()) {
        conn.execute(
            "DELETE FROM files_on_server WHERE sha256 IN (?1, ?2)",
            params![row.sha256, row.thumb_sha256],
        )?;
    }
    for row in rows {
        conn.execute(
            "DELETE FROM attachments WHERE id = ?1 AND note_id = ?2",
            params![row.id, note],
        )?;
    }
    for row in rows {
        conn.execute(
            "INSERT INTO attachments (id, note_id, file_name, mime_type, size, sha256, created_at,
                 thumb_sha256, epoch, removed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                row.id,
                note,
                row.file_name,
                row.mime_type,
                row.size,
                row.sha256,
                row.created_at,
                row.thumb_sha256,
                row.epoch,
                row.removed_at
            ],
        )?;
    }
    Ok(())
}

/// `name` if free among `taken`, else `stem 2.ext`, `stem 3.ext`, ...
pub(crate) fn free_name(name: &str, taken: &HashSet<String>) -> String {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let mut candidate = name.to_owned();
    let mut n = 2;
    while taken.contains(&candidate) {
        candidate = format!("{stem} {n}{ext}");
        n += 1;
    }
    candidate
}

/// A file the notes use, see [`Store::files_in_use`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileUse {
    /// Hex SHA-256 of the content.
    pub sha256: String,
    /// Its size when known (thumbnails do not say theirs).
    pub size: Option<i64>,
    /// A name and type it is attached under, which a standard server
    /// checks against its rules.
    pub name: String,
    /// See `name`.
    pub mime: String,
}

/// A file the server holds that no note uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Released {
    /// Hex SHA-256 of the content.
    pub sha256: String,
    /// Its address on the server, when known.
    pub address: Option<String>,
}

/// How the files travel.
#[derive(Debug)]
pub enum FileKeying {
    /// As they are: the server encrypts them.
    Plain,
    /// Sealed with this files key.
    Sealed(app_e2ee::FilesKey),
}

impl Store {
    /// Records that a file is attached to a note.
    ///
    /// # Errors
    ///
    /// Fails if the note does not exist, the name is empty or the hash is not
    /// lowercase hex SHA-256.
    pub fn add_attachment(
        &mut self,
        note: NoteId,
        file_name: &str,
        mime_type: &str,
        size: i64,
        sha256: &str,
    ) -> Result<Attachment> {
        let file_name = clean_text(file_name).trim().to_owned();
        if file_name.is_empty() {
            return Err(Error::Invalid("an attachment needs a file name".into()));
        }
        if !is_sha256_hex(sha256) || size < 0 {
            return Err(Error::Invalid(
                "attachment hash or size is malformed".into(),
            ));
        }
        self.note(note)?;
        let taken: HashSet<String> = self
            .list_attachments(note)?
            .into_iter()
            .map(|a| a.file_name)
            .collect();
        let now = self.now();
        let attachment = Attachment {
            id: AttachmentId::new(),
            note_id: note,
            file_name: free_name(&file_name, &taken),
            mime_type: mime_type.trim().to_owned(),
            size,
            sha256: sha256.to_owned(),
            created_at: now,
            thumb_sha256: None,
        };
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO attachments (id, note_id, file_name, mime_type, size, sha256, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                attachment.id,
                attachment.note_id,
                attachment.file_name,
                attachment.mime_type,
                attachment.size,
                attachment.sha256,
                attachment.created_at
            ],
        )?;
        crate::sync::finish(tx, &mut self.docs, now)?;
        Ok(attachment)
    }

    /// Gives an attachment its preview image.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] if it does not exist, [`Error::Invalid`] for a
    /// malformed hash.
    pub fn set_attachment_thumb(&mut self, id: AttachmentId, sha256: &str) -> Result<()> {
        if !is_sha256_hex(sha256) {
            return Err(Error::Invalid("thumbnail hash is malformed".into()));
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        if tx.execute(
            "UPDATE attachments SET thumb_sha256 = ?2 WHERE id = ?1 AND removed_at IS NULL",
            params![id, sha256],
        )? == 0
        {
            return Err(Error::NotFound("attachment"));
        }
        crate::sync::finish(tx, &mut self.docs, now)
    }

    /// Hashes of every file an attachment refers to, removed ones not yet
    /// forgotten included, to clean up unused files.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn referenced_blobs(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT sha256 FROM attachments
             UNION SELECT thumb_sha256 FROM attachments WHERE thumb_sha256 IS NOT NULL",
        )?;
        let set = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<HashSet<String>>>()?;
        Ok(set)
    }

    /// Hashes of the files attachments not removed refer to: what a device
    /// with no other device to tell of a removal keeps.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn kept_blobs(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT sha256 FROM attachments WHERE removed_at IS NULL
             UNION SELECT thumb_sha256 FROM attachments
             WHERE removed_at IS NULL AND thumb_sha256 IS NOT NULL",
        )?;
        let set = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<HashSet<String>>>()?;
        Ok(set)
    }

    /// The key that encrypts attachment files, created on first use. It lives
    /// in the database, so it is as protected as the notes.
    ///
    /// # Errors
    ///
    /// Fails on database or randomness errors.
    pub(crate) fn blob_key(&mut self) -> Result<Zeroizing<[u8; 32]>> {
        use rusqlite::OptionalExtension;
        let stored: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT value FROM vault_meta WHERE key = 'blob_key'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(bytes) = stored {
            let array: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| Error::Corrupt("blob key has the wrong length".into()))?;
            return Ok(Zeroizing::new(array));
        }
        let key = Zeroizing::new(random_bytes::<32>()?);
        self.conn.execute(
            "INSERT INTO vault_meta (key, value) VALUES ('blob_key', ?1)",
            [key.as_slice()],
        )?;
        Ok(key)
    }

    /// Attachments of a note, oldest first.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn list_attachments(&self, note: NoteId) -> Result<Vec<Attachment>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM attachments WHERE note_id = ?1 AND removed_at IS NULL
             ORDER BY created_at, id"
        ))?;
        let rows = stmt
            .query_map([note], attachment_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Every attachment of the notes not in the trash, newest first, each
    /// with its note's title.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn all_attachments(&self) -> Result<Vec<NoteAttachment>> {
        let columns = COLUMNS
            .split(", ")
            .map(|c| format!("a.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {columns}, n.title FROM attachments a JOIN notes n ON n.id = a.note_id
             WHERE a.removed_at IS NULL AND n.trashed_at IS NULL
             ORDER BY a.created_at DESC, a.id"
        ))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(NoteAttachment {
                    attachment: attachment_from_row(row)?,
                    note_title: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Detaches a file from its note. Its description stays, marked
    /// removed, until [`Store::forget_removed_files`]: other devices learn
    /// of the removal and the file is not lost at once.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] if it does not exist.
    pub fn remove_attachment(&mut self, id: AttachmentId) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        if tx.execute(
            "UPDATE attachments SET removed_at = ?2 WHERE id = ?1 AND removed_at IS NULL",
            params![id, now],
        )? == 0
        {
            return Err(Error::NotFound("attachment"));
        }
        crate::sync::finish(tx, &mut self.docs, now)
    }

    /// Forgets attachments removed more than `keep` milliseconds ago. Their
    /// descriptions in the documents still say they were removed, so nothing
    /// is recorded. Returns how many went.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn forget_removed_files(&mut self, keep: i64) -> Result<usize> {
        let cutoff = self.now().saturating_sub(keep);
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO sync_quiet (on_) VALUES (1)", [])?;
        let gone = tx.execute(
            "DELETE FROM attachments WHERE removed_at IS NOT NULL AND removed_at < ?1",
            [cutoff],
        )?;
        tx.execute("DELETE FROM sync_quiet", [])?;
        tx.commit()?;
        Ok(gone)
    }

    /// Every file the notes use and are attached to, trashed notes
    /// included, thumbnails too. What is not on this device waits to be
    /// fetched, what is not on the server to be uploaded.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn files_in_use(&self) -> Result<Vec<FileUse>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.sha256, MAX(a.size), MIN(a.file_name), MIN(a.mime_type)
             FROM attachments a JOIN notes n ON n.id = a.note_id
             WHERE a.removed_at IS NULL GROUP BY a.sha256
             UNION
             SELECT a.thumb_sha256, NULL, 'thumbnail.webp', 'image/webp'
             FROM attachments a JOIN notes n ON n.id = a.note_id
             WHERE a.removed_at IS NULL AND a.thumb_sha256 IS NOT NULL
             GROUP BY a.thumb_sha256
             ORDER BY 1",
        )?;
        let mut uses: Vec<FileUse> = stmt
            .query_map([], |row| {
                Ok(FileUse {
                    sha256: row.get(0)?,
                    size: row.get(1)?,
                    name: row.get(2)?,
                    mime: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // a file that is also some note's thumbnail keeps its known size
        uses.dedup_by(|b, a| {
            let same = a.sha256 == b.sha256;
            if same && a.size.is_none() {
                a.size = b.size;
                a.name.clone_from(&b.name);
                a.mime.clone_from(&b.mime);
            }
            same
        });
        Ok(uses)
    }

    /// Files the notes use that the server does not hold yet.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn files_to_upload(&self) -> Result<Vec<FileUse>> {
        let sent = self.files_on_server()?;
        // a refusal is tried again after a day: the rules may have changed
        let since = self.now().saturating_sub(crate::clock::DAY);
        let refused: HashSet<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT sha256 FROM files_refused WHERE at > ?1")?;
            stmt.query_map([since], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        Ok(self
            .files_in_use()?
            .into_iter()
            .filter(|f| !sent.contains(&f.sha256) && !refused.contains(&f.sha256))
            .collect())
    }

    /// Notes that the server refused a file, with its reason (an error code
    /// such as `fileTooLarge`).
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn file_refused(&mut self, sha256: &str, code: &str) -> Result<()> {
        let now = self.now();
        self.conn.execute(
            "INSERT OR REPLACE INTO files_refused (space, sha256, code, at) VALUES ('', ?1, ?2, ?3)",
            params![sha256, code, now],
        )?;
        Ok(())
    }

    /// Files the server refused, by hash, with the reason, for the note
    /// that shows them.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn files_refused(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT sha256, code FROM files_refused ORDER BY sha256")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Attachments whose file the server refused, for the settings: the
    /// file's name, its note's title and the reason.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn refused_attachments(&self) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.file_name, n.title, r.code FROM files_refused r
             JOIN attachments a ON a.sha256 = r.sha256 AND a.removed_at IS NULL
             JOIN notes n ON n.id = a.note_id
             ORDER BY n.title, a.file_name",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// How the files travel: sealed for an e2ee account.
    ///
    /// # Errors
    ///
    /// Database errors, a stored key that does not read.
    pub fn file_keying(&self) -> Result<FileKeying> {
        Ok(match self.sync_master_key()? {
            Some(master) => FileKeying::Sealed(app_e2ee::files_key(&master)),
            None => FileKeying::Plain,
        })
    }

    /// Whether changes wait to be sent: files are only let go once
    /// everything here is on the server.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_pending(&self) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT EXISTS (SELECT 1 FROM sync_outbox)", [], |row| {
                row.get(0)
            })?)
    }

    /// Files the server holds and no note uses anymore.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn files_to_release(&self) -> Result<Vec<Released>> {
        let used: HashSet<String> = self.files_in_use()?.into_iter().map(|f| f.sha256).collect();
        let mut stmt = self
            .conn
            .prepare("SELECT sha256, address FROM files_on_server ORDER BY sha256")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Released {
                    sha256: row.get(0)?,
                    address: row.get::<_, Option<String>>(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .filter(|r| !used.contains(&r.sha256))
            .collect())
    }

    fn files_on_server(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT sha256 FROM files_on_server")?;
        let set = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        Ok(set)
    }

    /// Notes that the server holds a file, after an upload or a download.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn file_on_server(&mut self, sha256: &str, address: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO files_on_server (space, sha256, address) VALUES ('', ?1, ?2)",
            params![sha256, address],
        )?;
        Ok(())
    }

    /// Notes that the server let go of a file.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn file_released(&mut self, sha256: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM files_on_server WHERE sha256 = ?1", [sha256])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::clock::DAY;
    use crate::store::NewNote;
    use crate::store::test_support::store;

    const HASH: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
    const OTHER: &str = "60303ae22b998861bce3b28f33eec1be758a213c86c93c076dbe9f558c11c752";

    #[test]
    fn adds_lists_and_removes() {
        let (mut store, _) = store();
        let note = store.create_note(&NewNote::default()).unwrap();
        let a = store
            .add_attachment(note.id, "vines.png", "image/png", 1234, HASH)
            .unwrap();
        assert_eq!(store.list_attachments(note.id).unwrap(), vec![a.clone()]);
        store.remove_attachment(a.id).unwrap();
        assert!(store.list_attachments(note.id).unwrap().is_empty());
        assert!(store.remove_attachment(a.id).is_err());
    }

    #[test]
    fn lists_every_file_with_its_note_outside_the_trash() {
        let (mut store, _) = store();
        let garden = store
            .create_note(&NewNote {
                title: "Garden".into(),
                ..NewNote::default()
            })
            .unwrap();
        let old = store
            .create_note(&NewNote {
                title: "Old".into(),
                ..NewNote::default()
            })
            .unwrap();
        let a = store
            .add_attachment(garden.id, "vines.png", "image/png", 1234, HASH)
            .unwrap();
        store
            .add_attachment(old.id, "gone.pdf", "application/pdf", 9, OTHER)
            .unwrap();
        store.trash_note(old.id).unwrap();
        let all = store.all_attachments().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].attachment, a);
        assert_eq!(all[0].note_title, "Garden");
    }

    #[test]
    fn names_stay_unique_per_note() {
        let (mut store, _) = store();
        let note = store.create_note(&NewNote::default()).unwrap();
        let other = store.create_note(&NewNote::default()).unwrap();
        assert_eq!(
            store
                .add_attachment(note.id, "a.png", "image/png", 1, HASH)
                .unwrap()
                .file_name,
            "a.png"
        );
        let second = store
            .add_attachment(note.id, "a.png", "image/png", 1, HASH)
            .unwrap();
        assert_eq!(second.file_name, "a 2.png");
        assert_eq!(
            store
                .add_attachment(other.id, "a.png", "image/png", 1, HASH)
                .unwrap()
                .file_name,
            "a.png"
        );
        assert_eq!(store.referenced_blobs().unwrap().len(), 1);
        // a removed name is free again
        store.remove_attachment(second.id).unwrap();
        assert_eq!(
            store
                .add_attachment(note.id, "a.png", "image/png", 1, HASH)
                .unwrap()
                .file_name,
            "a 2.png"
        );
    }

    #[test]
    fn blob_key_is_created_once() {
        let (mut store, _) = store();
        let first = store.blob_key().unwrap();
        assert_eq!(*store.blob_key().unwrap(), *first);
    }

    #[test]
    fn rejects_bad_hashes_and_names() {
        let (mut store, _) = store();
        let note = store.create_note(&NewNote::default()).unwrap();
        assert!(
            store
                .add_attachment(note.id, "a", "x", 1, "nothex")
                .is_err()
        );
        assert!(store.add_attachment(note.id, " ", "x", 1, HASH).is_err());
        assert!(
            store
                .add_attachment(note.id, "a", "x", 1, &HASH.to_uppercase())
                .is_err()
        );
    }

    #[test]
    fn attachments_go_away_with_their_note() {
        let (mut store, _) = store();
        let note = store.create_note(&NewNote::default()).unwrap();
        store
            .add_attachment(note.id, "a.txt", "text/plain", 1, HASH)
            .unwrap();
        store.trash_note(note.id).unwrap();
        store.purge_note(note.id).unwrap();
        assert!(store.list_attachments(note.id).unwrap().is_empty());
        assert!(store.referenced_blobs().unwrap().is_empty());
    }

    #[test]
    fn removed_files_are_kept_a_while_then_forgotten() {
        let (mut store, clock) = store();
        let note = store.create_note(&NewNote::default()).unwrap();
        let a = store
            .add_attachment(note.id, "a.png", "image/png", 1, HASH)
            .unwrap();
        store.set_attachment_thumb(a.id, OTHER).unwrap();
        assert_eq!(store.kept_blobs().unwrap().len(), 2);
        store.remove_attachment(a.id).unwrap();
        assert_eq!(store.referenced_blobs().unwrap().len(), 2);
        assert!(store.kept_blobs().unwrap().is_empty());
        assert_eq!(store.forget_removed_files(30 * DAY).unwrap(), 0);
        clock.advance(31 * DAY);
        assert_eq!(store.forget_removed_files(30 * DAY).unwrap(), 1);
        assert!(store.referenced_blobs().unwrap().is_empty());
    }

    #[test]
    fn tracks_what_the_server_holds() {
        let (mut store, _) = store();
        let note = store.create_note(&NewNote::default()).unwrap();
        let a = store
            .add_attachment(note.id, "a.png", "image/png", 5, HASH)
            .unwrap();
        store.set_attachment_thumb(a.id, OTHER).unwrap();
        let wanted: Vec<_> = store
            .files_to_upload()
            .unwrap()
            .into_iter()
            .map(|f| (f.sha256, f.size))
            .collect();
        assert_eq!(
            wanted,
            vec![(OTHER.to_owned(), None), (HASH.to_owned(), Some(5))]
        );
        store.file_on_server(HASH, HASH).unwrap();
        store.file_on_server(OTHER, OTHER).unwrap();
        assert!(store.files_to_upload().unwrap().is_empty());
        assert!(store.files_to_release().unwrap().is_empty());
        // still used while the note is in the trash
        store.trash_note(note.id).unwrap();
        assert!(store.files_to_release().unwrap().is_empty());
        store.restore_note(note.id).unwrap();
        store.remove_attachment(a.id).unwrap();
        assert_eq!(store.files_to_release().unwrap().len(), 2);
        store.file_released(HASH).unwrap();
        let left: Vec<_> = store
            .files_to_release()
            .unwrap()
            .into_iter()
            .map(|r| r.sha256)
            .collect();
        assert_eq!(left, vec![OTHER.to_owned()]);
    }
}
