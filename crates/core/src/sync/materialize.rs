// SPDX-License-Identifier: AGPL-3.0-or-later

//! Documents into rows, after a change from another device. The rows end
//! as if the change had been made here, by the same rules:
//!
//! - A note whose folder is in the trash is in the trash with it, so a note
//!   added on one device to a folder trashed on another goes to the trash
//!   with that folder and comes back when it is restored.
//! - A note whose folder no longer exists anywhere sits at the top level.
//! - A note deleted for good on one device and edited on another, by an
//!   edit the first device had not seen, comes back to the trash with the
//!   edit. Nothing typed is ever lost to a delete. Its document is kept
//!   after the delete for that reason, whatever order changes arrive in.
//! - Before a note's text changes, the version history keeps what it was,
//!   as for an edit made here.

use std::collections::{HashMap, HashSet};

use loro::{LoroMap, LoroValue, TreeParentId, ValueOrContainer, VersionVector};
use rusqlite::{Connection, OptionalExtension, params};

use super::record::{bool_of, int_of, node_folder, text_of, value};
use super::{Docs, SPACE, doc_key};
use crate::blobs::is_sha256_hex;
use crate::clock::Timestamp;
use crate::error::Result;
use crate::id::{AttachmentId, FolderId, NoteId};
use crate::store::{
    FileRow, clean_text, file_rows, free_name, history_snapshot, reindex_note, replace_file_rows,
    replace_tags,
};

struct FolderState {
    id: FolderId,
    parent: Option<FolderId>,
    name: String,
    position: i64,
    created: Timestamp,
    updated: Timestamp,
    trashed: Option<Timestamp>,
}

/// Rebuilds a space's folders from its tree.
pub(super) fn folders(
    conn: &Connection,
    docs: &mut Docs,
    space_id: &str,
    now: Timestamp,
) -> Result<()> {
    let space = docs.get(conn, &doc_key(space_id, SPACE))?;
    let tree = space.get_tree("folders");
    // breadth first from the roots, so a parent row exists before its children
    let mut states = Vec::new();
    let mut queue: Vec<(TreeParentId, Option<FolderId>)> = vec![(TreeParentId::Root, None)];
    while let Some((parent, parent_id)) = queue.pop() {
        let children: Vec<(loro::TreeID, FolderId)> = tree
            .children(parent)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|n| {
                node_folder(&tree, n)
                    .and_then(|f| f.parse().ok())
                    .map(|f| (n, f))
            })
            .collect();
        for (position, (node, id)) in children.into_iter().enumerate() {
            let Ok(meta) = tree.get_meta(node) else {
                continue;
            };
            states.push(FolderState {
                id,
                parent: parent_id,
                name: text_of(&meta, "name").unwrap_or_default(),
                position: i64::try_from(position).unwrap_or(i64::MAX),
                created: int_of(&meta, "created").unwrap_or(0),
                updated: int_of(&meta, "updated").unwrap_or(0),
                trashed: int_of(&meta, "trashed"),
            });
            queue.push((TreeParentId::Node(node), Some(id)));
        }
    }
    // the queue is a stack: sort by depth so parents still come first
    let depth_of = {
        let parents: HashMap<FolderId, Option<FolderId>> =
            states.iter().map(|s| (s.id, s.parent)).collect();
        move |mut id: FolderId| {
            let mut d = 0;
            while let Some(Some(p)) = parents.get(&id) {
                d += 1;
                id = *p;
                if d > 10_000 {
                    break;
                }
            }
            d
        }
    };
    states.sort_by_key(|s| depth_of(s.id));
    let keep: HashSet<FolderId> = states.iter().map(|s| s.id).collect();
    for s in &states {
        if s.trashed.is_some() {
            let was_trashed: Option<Option<Timestamp>> = conn
                .query_row(
                    "SELECT trashed_at FROM folders WHERE id = ?1",
                    [s.id],
                    |row| row.get(0),
                )
                .optional()?;
            if !matches!(was_trashed, Some(Some(_))) {
                trash_arrived(conn, "folder", &s.id.to_string(), now)?;
            }
        }
        conn.execute(
            "INSERT INTO folders (id, parent_id, name, position, created_at, updated_at, trashed_at, space_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET parent_id = excluded.parent_id, name = excluded.name,
                 position = excluded.position, created_at = excluded.created_at,
                 updated_at = excluded.updated_at, trashed_at = excluded.trashed_at,
                 space_id = excluded.space_id
             WHERE (folders.parent_id IS NOT excluded.parent_id) OR folders.name != excluded.name
                 OR folders.position != excluded.position OR folders.created_at != excluded.created_at
                 OR folders.updated_at != excluded.updated_at
                 OR (folders.trashed_at IS NOT excluded.trashed_at)
                 OR folders.space_id != excluded.space_id",
            params![
                s.id,
                s.parent,
                s.name,
                s.position,
                s.created,
                s.updated,
                s.trashed,
                space_id
            ],
        )?;
    }
    let existing = {
        let mut stmt = conn.prepare("SELECT id FROM folders WHERE space_id = ?1")?;
        stmt.query_map([space_id], |row| row.get::<_, FolderId>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for id in existing {
        if !keep.contains(&id) {
            conn.execute("DELETE FROM folders WHERE id = ?1", [id])?;
        }
    }
    Ok(())
}

/// Whether a note's document holds changes the deleting device had not seen.
fn edited_after(doc_vv: &VersionVector, seen: &[u8]) -> bool {
    let Ok(seen) = VersionVector::decode(seen) else {
        return true;
    };
    doc_vv
        .iter()
        .any(|(peer, counter)| seen.get(peer).copied().unwrap_or(0) < *counter)
}

fn entry_of(
    docs: &mut Docs,
    conn: &Connection,
    space_id: &str,
    note: NoteId,
) -> Result<Option<LoroMap>> {
    let space = docs.get(conn, &doc_key(space_id, SPACE))?;
    match space.get_map("notes").get(&note.to_string()) {
        Some(ValueOrContainer::Container(loro::Container::Map(m))) => Ok(Some(m)),
        _ => Ok(None),
    }
}

fn delete_row(conn: &Connection, space_id: &str, note: NoteId) -> Result<()> {
    conn.execute(
        "DELETE FROM notes_fts WHERE rowid = (SELECT rowid FROM notes WHERE id = ?1 AND space_id = ?2)",
        params![note, space_id],
    )?;
    conn.execute(
        "DELETE FROM notes WHERE id = ?1 AND space_id = ?2",
        params![note, space_id],
    )?;
    conn.execute(
        "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM note_tags)",
        [],
    )?;
    Ok(())
}

/// Notes when an item trashed elsewhere arrived here, see `trash_arrived`.
fn trash_arrived(conn: &Connection, kind: &str, id: &str, now: Timestamp) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO trash_arrived (kind, id, at) VALUES (?1, ?2, ?3)",
        params![kind, id, now],
    )?;
    Ok(())
}

/// Tag spellings of a note's entry, sorted.
fn tags_of(entry: &LoroMap) -> Vec<String> {
    match entry.get("tags") {
        Some(ValueOrContainer::Container(loro::Container::Map(m))) => {
            let mut names: Vec<String> = m.keys().filter_map(|k| text_of(&m, &k)).collect();
            names.sort_by_key(|n| n.to_lowercase());
            names
        }
        _ => Vec::new(),
    }
}

/// The attachments an entry describes, by id.
fn file_entries(entry: &LoroMap, note: NoteId) -> Vec<FileRow> {
    let Some(ValueOrContainer::Container(loro::Container::Map(files))) = entry.get("files") else {
        return Vec::new();
    };
    let mut out: Vec<FileRow> = files
        .keys()
        .filter_map(|key| {
            let Some(ValueOrContainer::Container(loro::Container::Map(f))) = files.get(&key) else {
                return None;
            };
            Some(FileRow {
                id: key.parse().ok()?,
                note_id: note,
                file_name: text_of(&f, "name")?,
                mime_type: text_of(&f, "mime").unwrap_or_default(),
                size: int_of(&f, "size").unwrap_or(0),
                // only hashes ever name files on disk, see blobs::is_sha256_hex
                sha256: text_of(&f, "sha256").filter(|h| is_sha256_hex(h))?,
                created_at: int_of(&f, "created").unwrap_or(0),
                thumb_sha256: text_of(&f, "thumb").filter(|h| is_sha256_hex(h)),
                epoch: int_of(&f, "epoch"),
                removed_at: int_of(&f, "removed"),
            })
        })
        .collect();
    out.sort_by_key(|f| f.id);
    out
}

/// Rebuilds a note's attachment rows. Two devices may attach a file of the
/// same name at once: the earlier keeps the name, the later becomes
/// `name 2`, the same on every device. A removed attachment this device
/// never had is not brought in, there is nothing to keep it for.
fn files(conn: &Connection, entry: &LoroMap, note: NoteId) -> Result<()> {
    let existing = file_rows(conn, note)?;
    let had: HashSet<AttachmentId> = existing.iter().map(|f| f.id).collect();
    let mut wanted: Vec<FileRow> = file_entries(entry, note)
        .into_iter()
        .filter(|f| f.removed_at.is_none() || had.contains(&f.id))
        .collect();
    let described: HashSet<AttachmentId> = wanted.iter().map(|f| f.id).collect();
    // rows the document does not hold yet keep their names
    let mut taken: HashSet<String> = existing
        .iter()
        .filter(|f| !described.contains(&f.id) && f.removed_at.is_none())
        .map(|f| f.file_name.clone())
        .collect();
    let mut by_age: Vec<&mut FileRow> = wanted
        .iter_mut()
        .filter(|f| f.removed_at.is_none())
        .collect();
    by_age.sort_by_key(|f| (f.created_at, f.id));
    for f in by_age {
        f.file_name = free_name(&f.file_name, &taken);
        taken.insert(f.file_name.clone());
    }
    let current: Vec<&FileRow> = existing
        .iter()
        .filter(|f| described.contains(&f.id))
        .collect();
    if current.len() != wanted.len() || current.iter().zip(&wanted).any(|(a, b)| **a != *b) {
        replace_file_rows(conn, note, &wanted)?;
    }
    Ok(())
}

/// A deletion for good from elsewhere: the row goes, and the time it
/// arrived is noted the first time only, so a rebuild of a note long gone
/// does not start its time again.
fn deleted_here(conn: &Connection, space_id: &str, id: NoteId, now: Timestamp) -> Result<()> {
    let had_row: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM notes WHERE id = ?1 AND space_id = ?2)",
        params![id, space_id],
        |row| row.get(0),
    )?;
    delete_row(conn, space_id, id)?;
    if had_row {
        super::deletion_arrived(conn, space_id, id, now)?;
    }
    Ok(())
}

/// A folder that still exists here, and when it went to the trash.
fn folder_state(
    conn: &Connection,
    folder: Option<FolderId>,
) -> Result<Option<(FolderId, Option<Timestamp>)>> {
    let Some(f) = folder else {
        return Ok(None);
    };
    Ok(conn
        .query_row(
            "SELECT id, trashed_at FROM folders WHERE id = ?1",
            [f],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

/// The note's row belongs to another space here: that space keeps it.
fn lives_elsewhere(conn: &Connection, space_id: &str, id: NoteId) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM notes WHERE id = ?1 AND space_id != ?2)",
        params![id, space_id],
        |row| row.get(0),
    )?)
}

/// Rebuilds one note's row from its document and its entry in a space.
pub(super) fn note(
    conn: &Connection,
    docs: &mut Docs,
    space_id: &str,
    id: NoteId,
    now: Timestamp,
) -> Result<()> {
    // the space has not told where this note goes yet, its row waits
    let Some(entry) = entry_of(docs, conn, space_id, id)? else {
        return Ok(());
    };
    if lives_elsewhere(conn, space_id, id)? {
        return Ok(());
    }
    let key = doc_key(space_id, &id.to_string());
    let has_doc = docs.stored(conn, &key)?;
    let doc = docs.get(conn, &key)?;
    let mut trashed = int_of(&entry, "trashed");
    let mut edited_while_deleted = false;

    if let Some(purged) = int_of(&entry, "purged") {
        let seen = match value(&entry, "seen") {
            Some(LoroValue::Binary(b)) => b.to_vec(),
            _ => Vec::new(),
        };
        if !has_doc || !edited_after(&doc.oplog_vv(), &seen) {
            // the document stays: an edit from elsewhere may still arrive,
            // and it needs the text it was made on
            deleted_here(conn, space_id, id, now)?;
            return Ok(());
        }
        // edited elsewhere after the delete: back to the trash, with the edit
        trashed = Some(trashed.unwrap_or(purged));
        edited_while_deleted = true;
    }

    let title = clean_text(&doc.get_text("title").to_string());
    let body = clean_text(&doc.get_text("body").to_string());
    let folder: Option<FolderId> = text_of(&entry, "folder").and_then(|f| f.parse().ok());
    // a folder gone everywhere leaves its notes at the top level
    let folder_state = folder_state(conn, folder)?;
    let folder = folder_state.map(|(f, _)| f);
    if trashed.is_none() {
        trashed = folder_state.and_then(|(_, t)| t);
    }
    let pinned = bool_of(&entry, "pinned");
    let created = int_of(&entry, "created").unwrap_or(now);
    let updated = int_of(&entry, "updated").unwrap_or(created);
    let tags = tags_of(&entry);

    let current = crate::store::load_note(conn, id).ok();
    let content_changed = match &current {
        Some(row) => {
            if row.title != title || row.body != body {
                history_snapshot(conn, row, now)?;
                true
            } else {
                false
            }
        }
        None => true,
    };
    match &current {
        None => {
            conn.execute(
                "INSERT INTO notes (id, folder_id, title, body, pinned, created_at, updated_at, trashed_at, space_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![id, folder, title, body, pinned, created, updated, trashed, space_id],
            )?;
        }
        Some(row) => {
            if content_changed
                || row.folder_id != folder
                || row.pinned != pinned
                || row.created_at != created
                || row.updated_at != updated
                || row.trashed_at != trashed
            {
                conn.execute(
                    "UPDATE notes SET folder_id = ?2, title = ?3, body = ?4, pinned = ?5,
                         created_at = ?6, updated_at = ?7, trashed_at = ?8 WHERE id = ?1",
                    params![id, folder, title, body, pinned, created, updated, trashed],
                )?;
            }
        }
    }
    let tags_changed = current.as_ref().is_none_or(|row| {
        let mut a: Vec<String> = row.tags.iter().map(|t| t.to_lowercase()).collect();
        let mut b: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
        a.sort();
        b.sort();
        a != b
    });
    if tags_changed {
        replace_tags(conn, id, &tags, now)?;
    }
    files(conn, &entry, id)?;
    if trashed.is_some() && current.as_ref().is_none_or(|row| row.trashed_at.is_none()) {
        trash_arrived(conn, "note", &id.to_string(), now)?;
    }
    // a note in the trash cannot be edited here: new text in it was written
    // elsewhere while it was deleted
    let was_trashed = current.as_ref().is_some_and(|row| row.trashed_at.is_some());
    if trashed.is_some() && content_changed && (edited_while_deleted || was_trashed) {
        super::conflict::record_edited_after_delete(conn, id, now)?;
    }
    if content_changed || tags_changed {
        reindex_note(conn, id)?;
    }
    Ok(())
}

/// What a space import changed.
pub(super) struct Touched {
    /// The folder tree.
    pub folders: bool,
    /// Notes whose entry changed.
    pub notes: HashSet<NoteId>,
    /// Some note's tags.
    pub tags: bool,
}

/// Reads what an import changed from Loro's diff. Every container sits on a
/// path from a root: `notes` then a note id (the entry, a mergeable map
/// Loro names `🤝:$notes>id`) then `tags`, or the `folders` tree. `None`
/// when a path is not one of those: rebuild everything.
pub(super) fn touched(space: &loro::LoroDoc, before: &loro::Frontiers) -> Option<Touched> {
    let after = space.state_frontiers();
    let batch = space.diff(before, &after).ok()?;
    let mut out = Touched {
        folders: false,
        notes: HashSet::new(),
        tags: false,
    };
    for (cid, diff) in batch.iter() {
        let path = space.get_path_to_container(cid)?;
        let first = match &path.first()?.0 {
            loro::ContainerID::Root { name, .. } => name.to_string(),
            loro::ContainerID::Normal { .. } => return None,
        };
        match first.as_str() {
            "folders" => out.folders = true,
            "notes" if path.len() == 1 => {
                // entries added or replaced
                let loro::event::Diff::Map(map) = diff else {
                    return None;
                };
                out.notes
                    .extend(map.updated.keys().filter_map(|k| k.parse::<NoteId>().ok()));
                out.tags = true;
            }
            "notes" => {
                let loro::Index::Key(key) = &path.get(1)?.1 else {
                    return None;
                };
                out.notes.insert(key.parse().ok()?);
                if matches!(path.get(2), Some((_, loro::Index::Key(k))) if k.as_str() == "tags") {
                    out.tags = true;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Rebuilds the given notes of a space, then the tag spellings if tags
/// changed.
pub(super) fn some(
    conn: &Connection,
    docs: &mut Docs,
    space_id: &str,
    touched: &Touched,
    now: Timestamp,
) -> Result<()> {
    for id in &touched.notes {
        note(conn, docs, space_id, *id, now)?;
    }
    if touched.tags {
        tag_spellings(conn, docs)?;
    }
    Ok(())
}

/// Rebuilds everything a space says: folders, then every note in it.
pub(super) fn all(
    conn: &Connection,
    docs: &mut Docs,
    space_id: &str,
    now: Timestamp,
) -> Result<()> {
    folders(conn, docs, space_id, now)?;
    let space = docs.get(conn, &doc_key(space_id, SPACE))?;
    let ids: Vec<NoteId> = space
        .get_map("notes")
        .keys()
        .filter_map(|k| k.parse().ok())
        .collect();
    for id in ids {
        note(conn, docs, space_id, id, now)?;
    }
    tag_spellings(conn, docs)
}

/// A tag is unique ignoring case and keeps one spelling. Two devices may
/// each have typed it first in another case: every device then settles on
/// the same spelling, the first in plain sort order among those in use.
pub(super) fn tag_spellings(conn: &Connection, docs: &mut Docs) -> Result<()> {
    let mut best: HashMap<String, String> = HashMap::new();
    for space_id in super::spaces_here(conn)? {
        let space = docs.get(conn, &doc_key(&space_id, SPACE))?;
        spellings_in(&space, &mut best);
    }
    let existing = {
        let mut stmt = conn.prepare("SELECT id, name FROM tags")?;
        stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (id, name) in existing {
        if let Some(wanted) = best.get(&name.to_lowercase())
            && *wanted != name
        {
            conn.execute(
                "UPDATE tags SET name = ?2 WHERE id = ?1",
                params![id, wanted],
            )?;
            let notes = {
                let mut stmt = conn.prepare("SELECT note_id FROM note_tags WHERE tag_id = ?1")?;
                stmt.query_map([&id], |row| row.get::<_, NoteId>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for note in notes {
                reindex_note(conn, note)?;
            }
        }
    }
    Ok(())
}

/// The first spelling of each tag among a space's entries.
fn spellings_in(space: &loro::LoroDoc, best: &mut HashMap<String, String>) {
    let entries = space.get_map("notes");
    for key in entries.keys() {
        let Some(ValueOrContainer::Container(loro::Container::Map(entry))) = entries.get(&key)
        else {
            continue;
        };
        // deleted notes count too: one may come back with its tags, and
        // every device holds the same entries, so all agree either way
        let Some(ValueOrContainer::Container(loro::Container::Map(tags))) = entry.get("tags")
        else {
            continue;
        };
        for tag in tags.keys() {
            if let Some(name) = text_of(&tags, &tag) {
                best.entry(tag.to_string())
                    .and_modify(|b| {
                        if name < *b {
                            b.clone_from(&name);
                        }
                    })
                    .or_insert(name);
            }
        }
    }
}
