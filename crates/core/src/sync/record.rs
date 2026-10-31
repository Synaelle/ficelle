// SPDX-License-Identifier: AGPL-3.0-or-later

//! Rows into documents: after a local write, the documents are brought to
//! what the rows now say. Only fields that differ are written, so recording
//! rows that already match the documents changes nothing, and a field one
//! device did not touch never overwrites another device's change to it.

use std::collections::HashMap;

use loro::{LoroDoc, LoroMap, LoroTree, LoroValue, TreeID, TreeParentId, ValueOrContainer};
use rusqlite::Connection;

use super::{Docs, SPACE, doc_key, set_text, sync_error};
use crate::clock::Timestamp;
use crate::error::Result;
use crate::id::{FolderId, NoteId};

/// A plain value of a map, `None` when absent or a container.
pub(super) fn value(map: &LoroMap, key: &str) -> Option<LoroValue> {
    match map.get(key) {
        Some(ValueOrContainer::Value(v)) => Some(v),
        _ => None,
    }
}

pub(super) fn text_of(map: &LoroMap, key: &str) -> Option<String> {
    match value(map, key) {
        Some(LoroValue::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

pub(super) fn int_of(map: &LoroMap, key: &str) -> Option<i64> {
    match value(map, key) {
        Some(LoroValue::I64(n)) => Some(n),
        _ => None,
    }
}

pub(super) fn bool_of(map: &LoroMap, key: &str) -> bool {
    matches!(value(map, key), Some(LoroValue::Bool(true)))
}

/// Writes a value unless the map already holds it.
fn put(map: &LoroMap, key: &str, new: LoroValue) -> Result<()> {
    if value(map, key).as_ref() != Some(&new) {
        map.insert(key, new).map_err(sync_error)?;
    }
    Ok(())
}

fn opt_int(v: Option<i64>) -> LoroValue {
    v.map_or(LoroValue::Null, LoroValue::I64)
}

fn opt_text(v: Option<String>) -> LoroValue {
    v.map_or(LoroValue::Null, |s| LoroValue::String(s.into()))
}

/// The map of one note in the space, created on first use.
pub(super) fn entry(space: &LoroDoc, note: NoteId) -> Result<LoroMap> {
    space
        .get_map("notes")
        .ensure_mergeable_map(&note.to_string())
        .map_err(sync_error)
}

/// Records a note's row into a space, or its deletion from that space when
/// the row is gone or now belongs to another space. Returns whether its
/// tags changed, the only case tag spellings need a look.
pub(super) fn note(
    conn: &Connection,
    docs: &mut Docs,
    space_id: &str,
    id: NoteId,
    now: Timestamp,
) -> Result<bool> {
    let space = docs.get(conn, &doc_key(space_id, SPACE))?;
    let row = crate::store::load_note(conn, id).optional_note()?;
    let Some(row) = row.filter(|r| r.space_id == space_id) else {
        deleted(conn, docs, space_id, &space, id, now)?;
        return Ok(true);
    };
    let doc = docs.get(conn, &doc_key(space_id, &id.to_string()))?;
    set_text(&doc.get_text("title"), &row.title)?;
    set_text(&doc.get_text("body"), &row.body)?;
    doc.commit();

    let entry = entry(&space, id)?;
    put(
        &entry,
        "folder",
        opt_text(row.folder_id.map(|f| f.to_string())),
    )?;
    put(&entry, "pinned", LoroValue::Bool(row.pinned))?;
    put(&entry, "created", LoroValue::I64(row.created_at))?;
    put(&entry, "updated", LoroValue::I64(row.updated_at))?;
    put(&entry, "trashed", opt_int(row.trashed_at))?;
    if value(&entry, "purged").is_some_and(|v| v != LoroValue::Null) {
        put(&entry, "purged", LoroValue::Null)?;
    }
    // tags as a set keyed by the lowercased name, the value keeps the spelling
    let tags = entry.ensure_mergeable_map("tags").map_err(sync_error)?;
    let wanted: HashMap<String, String> = row
        .tags
        .iter()
        .map(|t| (t.to_lowercase(), t.clone()))
        .collect();
    let mut changed = false;
    for key in tags.keys().map(|k| k.to_string()).collect::<Vec<_>>() {
        if !wanted.contains_key(&key) {
            tags.delete(&key).map_err(sync_error)?;
            changed = true;
        }
    }
    for (key, name) in &wanted {
        let new = LoroValue::String(name.clone().into());
        if value(&tags, key).as_ref() != Some(&new) {
            tags.insert(key, new).map_err(sync_error)?;
            changed = true;
        }
    }
    files(conn, &entry, id)?;
    space.commit();
    Ok(changed)
}

/// A note's attachments as a map by attachment id. Each is a mergeable map,
/// so devices describing the same attachment merge field by field. Entries
/// without a row are left alone: rows only go once removed long enough, and
/// the entry keeps saying it was removed.
fn files(conn: &Connection, entry: &LoroMap, note: NoteId) -> Result<()> {
    let rows = crate::store::file_rows(conn, note)?;
    if rows.is_empty() && entry.get("files").is_none() {
        return Ok(());
    }
    let files = entry.ensure_mergeable_map("files").map_err(sync_error)?;
    for row in rows {
        let file = files
            .ensure_mergeable_map(&row.id.to_string())
            .map_err(sync_error)?;
        put(&file, "name", LoroValue::String(row.file_name.into()))?;
        put(&file, "mime", LoroValue::String(row.mime_type.into()))?;
        put(&file, "size", LoroValue::I64(row.size))?;
        put(&file, "sha256", LoroValue::String(row.sha256.into()))?;
        put(&file, "created", LoroValue::I64(row.created_at))?;
        put(&file, "thumb", opt_text(row.thumb_sha256))?;
        put(&file, "epoch", opt_int(row.epoch))?;
        put(&file, "removed", opt_int(row.removed_at))?;
    }
    Ok(())
}

/// A note deleted for good here. The space keeps a mark with the version of
/// the note's document at that moment: an edit made elsewhere that this
/// device had not seen brings the note back (to the trash) instead of being
/// lost.
fn deleted(
    conn: &Connection,
    docs: &mut Docs,
    space_id: &str,
    space: &LoroDoc,
    id: NoteId,
    now: Timestamp,
) -> Result<()> {
    let seen = docs
        .get(conn, &doc_key(space_id, &id.to_string()))?
        .oplog_vv()
        .encode();
    let entry = entry(space, id)?;
    put(&entry, "purged", LoroValue::I64(now))?;
    put(&entry, "seen", LoroValue::Binary(seen.into()))?;
    space.commit();
    super::deletion_arrived(conn, space_id, id, now)
}

/// The folder id a tree node stands for.
pub(super) fn node_folder(tree: &LoroTree, node: TreeID) -> Option<String> {
    tree.get_meta(node).ok().and_then(|m| text_of(&m, "id"))
}

/// Live nodes by the folder they stand for.
pub(super) fn nodes_by_folder(tree: &LoroTree) -> HashMap<String, TreeID> {
    tree.nodes()
        .into_iter()
        .filter(|n| !tree.is_node_deleted(n).unwrap_or(true))
        .filter_map(|n| node_folder(tree, n).map(|f| (f, n)))
        .collect()
}

struct FolderRow {
    id: FolderId,
    parent: Option<FolderId>,
    name: String,
    position: i64,
    created: Timestamp,
    updated: Timestamp,
    trashed: Option<Timestamp>,
}

/// Records the whole folders table into the tree: new folders, moves,
/// renames, order among siblings, trash, deletions.
pub(super) fn folders(conn: &Connection, docs: &mut Docs, space_id: &str) -> Result<()> {
    let space = docs.get(conn, &doc_key(space_id, SPACE))?;
    let tree = space.get_tree("folders");
    // parents before children, so a new folder's parent node exists. A
    // space's top folders may sit in a folder of another space for the
    // device that moved them: they are at the top here
    let rows = {
        let mut stmt = conn.prepare(
            "WITH RECURSIVE depth(id, d) AS (
                 SELECT id, 0 FROM folders f WHERE space_id = ?1 AND (parent_id IS NULL
                     OR NOT EXISTS (SELECT 1 FROM folders p WHERE p.id = f.parent_id AND p.space_id = ?1))
                 UNION ALL
                 SELECT f.id, depth.d + 1 FROM folders f JOIN depth ON f.parent_id = depth.id
                 WHERE f.space_id = ?1
             )
             SELECT f.id, f.parent_id, f.name, f.position, f.created_at, f.updated_at, f.trashed_at
             FROM folders f JOIN depth ON depth.id = f.id
             ORDER BY depth.d, f.parent_id, f.position, f.id",
        )?;
        stmt.query_map([space_id], |row| {
            Ok(FolderRow {
                id: row.get(0)?,
                parent: row.get(1)?,
                name: row.get(2)?,
                position: row.get(3)?,
                created: row.get(4)?,
                updated: row.get(5)?,
                trashed: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut by_folder = nodes_by_folder(&tree);
    let parent_of = |by: &HashMap<String, TreeID>, p: Option<FolderId>| -> TreeParentId {
        match p.and_then(|p| by.get(&p.to_string()).copied()) {
            Some(node) => TreeParentId::Node(node),
            None => TreeParentId::Root,
        }
    };
    for row in &rows {
        let parent = parent_of(&by_folder, row.parent);
        let key = row.id.to_string();
        let node = if let Some(&node) = by_folder.get(&key) {
            if tree.parent(node) != Some(parent) {
                tree.mov(node, parent).map_err(sync_error)?;
            }
            node
        } else {
            let node = tree.create(parent).map_err(sync_error)?;
            tree.get_meta(node)
                .map_err(sync_error)?
                .insert("id", key.clone())
                .map_err(sync_error)?;
            by_folder.insert(key, node);
            node
        };
        let meta = tree.get_meta(node).map_err(sync_error)?;
        put(&meta, "name", LoroValue::String(row.name.clone().into()))?;
        put(&meta, "created", LoroValue::I64(row.created))?;
        put(&meta, "updated", LoroValue::I64(row.updated))?;
        put(&meta, "trashed", opt_int(row.trashed))?;
    }
    // order among siblings, as the rows' positions say
    let mut groups: HashMap<Option<FolderId>, Vec<&FolderRow>> = HashMap::new();
    for row in &rows {
        groups.entry(row.parent).or_default().push(row);
    }
    for (parent, mut children) in groups {
        children.sort_by_key(|r| (r.position, r.id));
        let wanted: Vec<TreeID> = children
            .iter()
            .filter_map(|r| by_folder.get(&r.id.to_string()).copied())
            .collect();
        let parent = parent_of(&by_folder, parent);
        let current: Vec<TreeID> = tree
            .children(parent)
            .unwrap_or_default()
            .into_iter()
            .filter(|n| wanted.contains(n))
            .collect();
        if current != wanted {
            for (index, node) in wanted.iter().enumerate() {
                tree.mov_to(*node, parent, index).map_err(sync_error)?;
            }
        }
    }
    // folders deleted for good
    let alive: std::collections::HashSet<String> = rows.iter().map(|r| r.id.to_string()).collect();
    for (folder, node) in nodes_by_folder(&tree) {
        if !alive.contains(&folder) && !tree.is_node_deleted(&node).unwrap_or(true) {
            tree.delete(node).map_err(sync_error)?;
        }
    }
    space.commit();
    Ok(())
}

/// `Option` for a lookup that fails with not found.
trait OptionalNote<T> {
    fn optional_note(self) -> Result<Option<T>>;
}

impl<T> OptionalNote<T> for Result<T> {
    fn optional_note(self) -> Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(crate::error::Error::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
