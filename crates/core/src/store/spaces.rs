// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared spaces left by older versions. Sharing notes left the app: a
//! vault that still holds a space shared with this account or by it makes
//! everything in it the person's own, once, when its notes open. Nobody
//! loses notes because sharing ended.
//!
//! The notes and folders are copied into the personal space with new ids,
//! with their text and its history, tags, pin, dates, attachments and
//! versions, folders as they were. Then every trace of the shared spaces
//! goes, quietly: nothing is sent to them, as to a space that ended.

use std::collections::HashMap;

use rusqlite::{Connection, params};

use super::{load_note, reindex};
use crate::clock::{Clock, SystemClock, Timestamp};
use crate::error::{Error, Result};
use crate::id::{AttachmentId, FolderId, NoteId};
use crate::sync::{Docs, doc_key};

/// Copies folders into the personal space with new ids, each tree's top at
/// the top. Returns the new id of each.
fn copy_folders(conn: &Connection, folders: &[FolderId]) -> Result<HashMap<FolderId, FolderId>> {
    let new_folders: HashMap<FolderId, FolderId> =
        folders.iter().map(|old| (*old, FolderId::new())).collect();
    for old in folders {
        let new = new_folders[old];
        let (parent, name, position, created, updated, trashed): (
            Option<FolderId>,
            String,
            i64,
            i64,
            i64,
            Option<i64>,
        ) = conn.query_row(
            "SELECT parent_id, name, position, created_at, updated_at, trashed_at FROM folders WHERE id = ?1",
            [old],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )?;
        let parent = parent.and_then(|p| new_folders.get(&p).copied());
        conn.execute(
            "INSERT INTO folders (id, parent_id, name, position, created_at, updated_at, trashed_at, space_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '')",
            params![new, parent, name, position, created, updated, trashed],
        )?;
    }
    Ok(new_folders)
}

/// Copies a note of a shared space into the personal space with a new id:
/// its text with the text's history, tags, pin, dates, versions and
/// attachments. The original row goes.
fn copy_note(
    conn: &Connection,
    docs: &mut Docs,
    source: &str,
    old: NoteId,
    new_folders: &HashMap<FolderId, FolderId>,
) -> Result<NoteId> {
    let new = NoteId::new();
    let note = load_note(conn, old)?;
    let from = docs.get(conn, &doc_key(source, &old.to_string()))?;
    let snapshot = from
        .export(loro::ExportMode::Snapshot)
        .map_err(|e| Error::Corrupt(format!("sync document: {e}")))?;
    let to = docs.get(conn, &new.to_string())?;
    to.import(&snapshot)
        .map_err(|e| Error::Corrupt(format!("sync document: {e}")))?;
    let folder = note.folder_id.and_then(|f| new_folders.get(&f).copied());
    conn.execute(
        "INSERT INTO notes (id, folder_id, title, body, pinned, created_at, updated_at, trashed_at, space_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '')",
        params![
            new,
            folder,
            note.title,
            note.body,
            note.pinned,
            note.created_at,
            note.updated_at,
            note.trashed_at
        ],
    )?;
    conn.execute(
        "INSERT INTO note_tags (note_id, tag_id) SELECT ?2, tag_id FROM note_tags WHERE note_id = ?1",
        params![old, new],
    )?;
    conn.execute(
        "UPDATE note_versions SET note_id = ?2 WHERE note_id = ?1",
        params![old, new],
    )?;
    // attachments get new ids too, as every attachment of a new note; the
    // epoch belonged to the space the files leave
    let files: Vec<AttachmentId> = {
        let mut stmt = conn.prepare("SELECT id FROM attachments WHERE note_id = ?1")?;
        stmt.query_map([old], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for file in files {
        conn.execute(
            "UPDATE attachments SET id = ?2, note_id = ?3, epoch = NULL WHERE id = ?1",
            params![file, AttachmentId::new(), new],
        )?;
    }
    conn.execute(
        "DELETE FROM notes_fts WHERE rowid = (SELECT rowid FROM notes WHERE id = ?1)",
        [old],
    )?;
    conn.execute("DELETE FROM notes WHERE id = ?1", [old])?;
    reindex(conn, new)?;
    Ok(new)
}

/// Everything in one shared space becomes the person's own. Returns the
/// new folders. Nothing is recorded in the space itself.
fn keep_all(
    conn: &Connection,
    docs: &mut Docs,
    space: &str,
    now: Timestamp,
) -> Result<Vec<FolderId>> {
    let roots: Vec<FolderId> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM folders WHERE space_id = ?1
               AND (parent_id IS NULL
                    OR parent_id NOT IN (SELECT id FROM folders WHERE space_id = ?1))",
        )?;
        stmt.query_map([space], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut folders = Vec::new();
    for root in roots {
        folders.extend(super::folders::subtree(conn, root)?);
    }
    let new_folders = copy_folders(conn, &folders)?;
    let notes: Vec<NoteId> = {
        let mut stmt = conn.prepare("SELECT id FROM notes WHERE space_id = ?1")?;
        stmt.query_map([space], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for old in notes {
        let new = copy_note(conn, docs, space, old, &new_folders)?;
        // the trash counts from now for what was in the space's trash: it
        // was never in this person's own
        conn.execute(
            "INSERT OR REPLACE INTO trash_arrived (kind, id, at)
             SELECT 'note', id, ?2 FROM notes WHERE id = ?1 AND trashed_at IS NOT NULL",
            params![new, now],
        )?;
    }
    for new in new_folders.values() {
        conn.execute(
            "INSERT OR REPLACE INTO trash_arrived (kind, id, at)
             SELECT 'folder', id, ?2 FROM folders WHERE id = ?1 AND trashed_at IS NOT NULL",
            params![new, now],
        )?;
    }
    // the copies are recorded in the personal space, the space records nothing
    conn.execute("DELETE FROM sync_touched WHERE space = ?1", [space])?;
    Ok(new_folders.into_values().collect())
}

/// Makes every shared space the vault holds the person's own, then removes
/// what sharing kept. Runs once, as the schema step that ends sharing.
///
/// # Errors
///
/// Database errors, a stored document that does not read. Nothing changes
/// then and the vault does not open.
pub(crate) fn fold_shared_spaces(conn: &Connection) -> Result<()> {
    let now = Clock::now(&SystemClock);
    let mut docs = Docs::default();
    // the spaces listed, and any a note or folder still names
    let spaces: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM spaces
             UNION SELECT space_id FROM notes WHERE space_id <> ''
             UNION SELECT space_id FROM folders WHERE space_id <> ''
             ORDER BY 1",
        )?;
        stmt.query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    if !spaces.is_empty() {
        let mut folders = Vec::new();
        for space in &spaces {
            folders.extend(keep_all(conn, &mut docs, space, now)?);
        }
        // folders show as tags: a note kept from a space read only never
        // had its folder's
        super::tags::tags_from_folders(conn, &folders, now)?;
        crate::sync::flush(conn, &mut docs, now)?;
    }
    conn.execute("INSERT INTO sync_quiet (on_) VALUES (1)", [])?;
    let dropped: Result<()> = (|| {
        // what is left of the spaces holds no note: theirs are copied, a
        // personal note never sits in a shared folder
        conn.execute("DELETE FROM folders WHERE space_id <> ''", [])?;
        super::tags::drop_unused_tags(conn)?;
        for table in [
            "sync_docs WHERE id",
            "sync_updates WHERE doc_id",
            "sync_outbox WHERE doc_id",
            "sync_waiting WHERE doc_id",
            "sync_rejected WHERE doc_id",
            "trash_arrived WHERE id",
        ] {
            conn.execute(&format!("DELETE FROM {table} LIKE '%/%'"), [])?;
        }
        conn.execute_batch(
            "DELETE FROM files_on_server WHERE space <> '';
             DELETE FROM files_refused WHERE space <> '';
             DELETE FROM vault_meta
                 WHERE key LIKE 'peer:%' OR key LIKE 'rejoin:%' OR key LIKE 'owner_key:%';
             DROP TABLE space_waiting;
             DROP TABLE space_trust;
             DROP TABLE space_keys;
             DROP TABLE spaces;
             DROP TABLE file_sources;",
        )?;
        Ok(())
    })();
    conn.execute("DELETE FROM sync_quiet", [])?;
    dropped
}
