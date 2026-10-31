// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sync: notes, folders and tags as CRDT documents (Loro).
//!
//! Two kinds of documents:
//! - one per note, holding its title and body as text, so two devices
//!   editing the same note merge character by character;
//! - one "space" per vault: the folder tree (a movable tree, so folders
//!   moved on two devices at once never end up inside each other) and, for
//!   each note, its folder, pin, tags, trash state and dates.
//!
//! The rows in `notes`, `folders` and `tags` stay the local view that lists
//! and search read. Local writes change the rows, triggers note what they
//! touched, and [`flush`] records it into the documents in the same
//! transaction. Changes from other devices go the other way: imported into
//! a document, then [`apply_remote`] rebuilds the rows from it. Both paths
//! meet in the documents, so every device ends with the same rows.
//!
//! Each local change is also queued in `sync_outbox` for the server.

pub(crate) mod conflict;
mod diff;
pub mod engine;
#[cfg(test)]
mod engine_tests;
pub mod files;
mod materialize;
mod record;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use loro::{ExportMode, LoroDoc, VersionVector};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::clock::Timestamp;
use crate::error::{Error, Result};
use crate::id::NoteId;

/// Id of the space document.
pub const SPACE: &str = "space";

/// Where a document of a space is stored: the personal space's keep their
/// bare id, a shared space's are `<space id>/<document id>`.
pub fn doc_key(space: &str, doc: &str) -> String {
    if space.is_empty() {
        doc.to_owned()
    } else {
        format!("{space}/{doc}")
    }
}

/// A stored key back to its space and document.
pub fn split_key(key: &str) -> (&str, &str) {
    key.split_once('/').unwrap_or(("", key))
}

/// The spaces this vault holds: the personal one, then the shared ones.
pub(crate) fn spaces_here(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM spaces ORDER BY id")?;
    let shared = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(std::iter::once(String::new()).chain(shared).collect())
}

/// Changes kept apart before they are folded into the stored snapshot.
const COMPACT_AFTER: i64 = 64;

/// Above this many bytes a body is diffed line by line, a character diff of
/// a very long text can take seconds.
const LINE_DIFF_ABOVE: usize = 20_000;

fn sync_error(err: impl std::fmt::Display) -> Error {
    Error::Corrupt(format!("sync document: {err}"))
}

struct Open {
    doc: LoroDoc,
    /// Version already written to `sync_updates`.
    saved: VersionVector,
}

/// The documents in use, loaded from the database on first use.
#[derive(Default)]
pub struct Docs {
    peer: Option<u64>,
    /// Shared spaces this vault dropped and holds again write under a peer
    /// id of their own, see [`Docs::peer_for`].
    space_peers: HashMap<String, Option<u64>>,
    open: HashMap<String, Open>,
}

impl std::fmt::Debug for Docs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Docs")
            .field("open", &self.open.len())
            .finish_non_exhaustive()
    }
}

/// One change waiting for the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// Order in the outbox, to acknowledge up to.
    pub seq: i64,
    /// Name of the change for the server, unique for good: this vault's
    /// peer id and the outbox number, which never repeats. Sending it again
    /// after a lost answer is then harmless.
    pub id: String,
    /// Document it belongs to: [`SPACE`] or a note id.
    pub doc_id: String,
    /// Loro update bytes.
    pub bytes: Vec<u8>,
    /// A whole document replacing its changes on the server up to this
    /// number.
    pub compacts: Option<i64>,
}

impl Docs {
    /// This device's id in the documents, made once per vault. Two devices
    /// never share one, so their edits never get mistaken for each other.
    fn peer(&mut self, conn: &Connection) -> Result<u64> {
        if let Some(peer) = self.peer {
            return Ok(peer);
        }
        let stored: Option<Vec<u8>> = conn
            .query_row(
                "SELECT value FROM vault_meta WHERE key = 'sync_peer'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let peer = if let Some(bytes) = stored {
            u64::from_le_bytes(
                bytes
                    .try_into()
                    .map_err(|_| Error::Corrupt("sync peer id".into()))?,
            )
        } else {
            // the top values are reserved by Loro
            let peer = u64::from_le_bytes(crate::crypto::random_bytes::<8>()?) >> 1;
            conn.execute(
                "INSERT INTO vault_meta (key, value) VALUES ('sync_peer', ?1)",
                [peer.to_le_bytes().to_vec()],
            )?;
            peer
        };
        self.peer = Some(peer);
        Ok(peer)
    }

    /// The peer id a document is written under. A shared space this vault
    /// dropped (removed, refused, left) was forgotten with this device's
    /// own changes in it: written again under the same id, new changes
    /// would reuse numbers the server already holds for other content.
    /// Dropping a space gives it a fresh id here for good.
    fn peer_for(&mut self, conn: &Connection, id: &str) -> Result<u64> {
        let Some((space, _)) = id.split_once('/') else {
            return self.peer(conn);
        };
        if !self.space_peers.contains_key(space) {
            let stored: Option<Vec<u8>> = conn
                .query_row(
                    "SELECT value FROM vault_meta WHERE key = ?1",
                    [format!("peer:{space}")],
                    |row| row.get(0),
                )
                .optional()?;
            let peer = stored
                .map(|bytes| {
                    bytes
                        .try_into()
                        .map(u64::from_le_bytes)
                        .map_err(|_| Error::Corrupt("space peer id".into()))
                })
                .transpose()?;
            self.space_peers.insert(space.to_owned(), peer);
        }
        match self.space_peers.get(space).copied().flatten() {
            Some(peer) => Ok(peer),
            None => self.peer(conn),
        }
    }

    /// The document, loaded from its snapshot and later changes if needed,
    /// empty if it was never stored. Loro documents are shared handles, the
    /// clone edits the same document.
    pub(crate) fn get(&mut self, conn: &Connection, id: &str) -> Result<LoroDoc> {
        if let Some(open) = self.open.get(id) {
            return Ok(open.doc.clone());
        }
        let peer = self.peer_for(conn, id)?;
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).map_err(sync_error)?;
        let snapshot: Option<Vec<u8>> = conn
            .query_row(
                "SELECT snapshot FROM sync_docs WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(snapshot) = snapshot {
            doc.import(&snapshot).map_err(sync_error)?;
        }
        let mut stmt =
            conn.prepare("SELECT bytes FROM sync_updates WHERE doc_id = ?1 ORDER BY seq")?;
        let updates = stmt
            .query_map([id], |row| row.get::<_, Vec<u8>>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for update in updates {
            doc.import(&update).map_err(sync_error)?;
        }
        // held back by Loro until what they build on arrives, which it only
        // keeps in memory
        let mut stmt =
            conn.prepare("SELECT bytes FROM sync_waiting WHERE doc_id = ?1 ORDER BY seq")?;
        let waiting = stmt
            .query_map([id], |row| row.get::<_, Vec<u8>>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for bytes in waiting {
            doc.import(&bytes).map_err(sync_error)?;
        }
        if split_key(id).1 == SPACE {
            doc.get_tree("folders").enable_fractional_index(0);
        }
        let saved = doc.oplog_vv();
        self.open.insert(
            id.to_owned(),
            Open {
                doc: doc.clone(),
                saved,
            },
        );
        Ok(doc)
    }

    /// Open documents holding changes not saved, for tests.
    #[cfg(test)]
    pub(crate) fn unsaved(&self) -> Vec<String> {
        self.open
            .iter()
            .filter(|(_, open)| open.doc.oplog_vv() != open.saved)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Writes what changed in a document since it was last saved. Local
    /// changes also go to the outbox.
    pub(crate) fn persist(
        &mut self,
        conn: &Connection,
        id: &str,
        local: bool,
        now: Timestamp,
    ) -> Result<()> {
        let Some(open) = self.open.get_mut(id) else {
            return Ok(());
        };
        let current = open.doc.oplog_vv();
        if current == open.saved {
            return Ok(());
        }
        let bytes = open
            .doc
            .export(ExportMode::updates(&open.saved))
            .map_err(sync_error)?;
        conn.execute(
            "INSERT INTO sync_updates (doc_id, bytes) VALUES (?1, ?2)",
            params![id, bytes],
        )?;
        if local {
            conn.execute(
                "INSERT INTO sync_outbox (doc_id, bytes, created_at) VALUES (?1, ?2, ?3)",
                params![id, bytes, now],
            )?;
        }
        open.saved = current;
        let pending: i64 = conn.query_row(
            "SELECT count(*) FROM sync_updates WHERE doc_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        if pending > COMPACT_AFTER {
            let snapshot = open.doc.export(ExportMode::Snapshot).map_err(sync_error)?;
            conn.execute(
                "INSERT INTO sync_docs (id, snapshot) VALUES (?1, ?2)
                 ON CONFLICT (id) DO UPDATE SET snapshot = excluded.snapshot",
                params![id, snapshot],
            )?;
            conn.execute("DELETE FROM sync_updates WHERE doc_id = ?1", [id])?;
        }
        Ok(())
    }

    /// Forgets the loaded documents, they reload from the database. Used
    /// when a transaction fails after documents were changed in memory.
    pub(crate) fn forget(&mut self) {
        self.open.clear();
        self.space_peers.clear();
    }

    /// Whether anything is stored for a document.
    pub(crate) fn stored(&self, conn: &Connection, id: &str) -> Result<bool> {
        if self.open.contains_key(id) {
            return Ok(true);
        }
        Ok(conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sync_docs WHERE id = ?1)
                 OR EXISTS (SELECT 1 FROM sync_updates WHERE doc_id = ?1)",
            [id],
            |row| row.get(0),
        )?)
    }
}

/// Sets a text to `value` with as few operations as a diff allows, so
/// concurrent edits elsewhere in the text survive.
fn set_text(text: &loro::LoroText, value: &str) -> Result<()> {
    if text.to_string() == value {
        return Ok(());
    }
    let options = loro::UpdateOptions::default();
    if value.len() > LINE_DIFF_ABOVE {
        text.update_by_line(value, options).map_err(sync_error)
    } else {
        text.update(value, options).map_err(sync_error)
    }
}

/// Merges two edits of the same text made from `base`, character by
/// character, as two devices editing at once would: an edit saved by a
/// screen that had not seen the other device's change yet keeps both.
///
/// # Errors
///
/// None in practice, Loro errors on documents made here.
pub(crate) fn merge_text(base: &str, theirs: &str, mine: &str) -> Result<String> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).map_err(sync_error)?;
    doc.get_text("t").insert(0, base).map_err(sync_error)?;
    doc.commit();
    let other = doc.fork();
    other.set_peer_id(2).map_err(sync_error)?;
    // when both inserted at the same place, the text being typed here comes
    // first: words typed at the end of a paragraph stay with it, the other
    // device's addition follows
    set_text(&doc.get_text("t"), mine)?;
    doc.commit();
    set_text(&other.get_text("t"), theirs)?;
    other.commit();
    doc.import(
        &other
            .export(ExportMode::all_updates())
            .map_err(sync_error)?,
    )
    .map_err(sync_error)?;
    Ok(doc.get_text("t").to_string())
}

#[cfg(test)]
mod merge_tests {
    use super::merge_text;

    #[test]
    fn both_edits_are_kept_and_typing_here_comes_first_at_the_same_place() {
        assert_eq!(
            merge_text(
                "one
two",
                "zero
one
two",
                "one
two
three"
            )
            .unwrap(),
            "zero
one
two
three"
        );
        assert_eq!(
            merge_text(
                "and more",
                "and more

A again",
                "and more last"
            )
            .unwrap(),
            "and more last

A again"
        );
        assert_eq!(merge_text("same", "same", "mine").unwrap(), "mine");
        assert_eq!(merge_text("a b c", "a c", "a b c d").unwrap(), "a c d");
    }
}

/// Records what the local writes of this transaction touched into the
/// documents, and queues it for the server.
///
/// # Errors
///
/// Database errors, or a stored document that does not read.
pub(crate) fn flush(conn: &Connection, docs: &mut Docs, now: Timestamp) -> Result<()> {
    let touched = {
        let mut stmt = conn.prepare("SELECT kind, space, id FROM sync_touched")?;
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    if touched.is_empty() {
        return Ok(());
    }
    conn.execute("DELETE FROM sync_touched", [])?;
    // a space shared with this account read only takes no change from here
    for (_, space, _) in &touched {
        let role: Option<String> = conn
            .query_row("SELECT role FROM spaces WHERE id = ?1", [space], |row| {
                row.get(0)
            })
            .optional()?;
        if role.as_deref() == Some("read") {
            return Err(Error::Invalid(
                "this is shared with you to read only".into(),
            ));
        }
    }
    let mut spaces: Vec<&str> = Vec::new();
    let mut folders_in: Vec<&str> = Vec::new();
    let mut tags_changed = false;
    for (kind, space, _) in &touched {
        if kind == "folders" && !folders_in.contains(&space.as_str()) {
            record::folders(conn, docs, space)?;
            folders_in.push(space);
        }
    }
    for (kind, space, id) in &touched {
        if kind != "note" {
            continue;
        }
        let note: NoteId = id
            .parse()
            .map_err(|_| Error::Corrupt("note id in sync_touched".into()))?;
        tags_changed |= record::note(conn, docs, space, note, now)?;
        docs.persist(conn, &doc_key(space, id), true, now)?;
        if !spaces.contains(&space.as_str()) {
            spaces.push(space);
        }
    }
    for space in &folders_in {
        if !spaces.contains(space) {
            spaces.push(space);
        }
    }
    for space in &spaces {
        docs.persist(conn, &doc_key(space, SPACE), true, now)?;
    }
    if !spaces.is_empty() {
        // positions and tag spellings follow the documents after local
        // changes as after remote ones, so devices never number folders or
        // spell a tag differently
        conn.execute("INSERT INTO sync_quiet (on_) VALUES (1)", [])?;
        let settled: Result<()> = (|| {
            for space in &folders_in {
                materialize::folders(conn, docs, space, now)?;
            }
            if tags_changed {
                materialize::tag_spellings(conn, docs)?;
            }
            Ok(())
        })();
        conn.execute("DELETE FROM sync_quiet", [])?;
        settled?;
    }
    Ok(())
}

/// Ends a write: records it, then commits. On failure the documents in
/// memory may be ahead of the database, so they are reloaded.
///
/// # Errors
///
/// As [`flush`], and commit errors.
pub(crate) fn finish(tx: Transaction<'_>, docs: &mut Docs, now: Timestamp) -> Result<()> {
    let done = flush(&tx, docs, now).and_then(|()| Ok(tx.commit()?));
    if done.is_err() {
        docs.forget();
    }
    done
}

/// Builds the documents of a vault from its rows: every folder and note is
/// recorded as if just written. Run once by the migration that adds sync.
///
/// # Errors
///
/// Database errors.
pub(crate) fn bootstrap(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO sync_touched (kind, space, id) SELECT 'note', space_id, id FROM notes",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO sync_touched (kind, space, id) VALUES ('folders', '', '*')",
        [],
    )?;
    let now = crate::clock::Clock::now(&crate::clock::SystemClock);
    flush(conn, &mut Docs::default(), now)
}

/// Records the attachments a vault already holds, once, when they start to
/// sync. Spaces shared read only are left out: their owner's devices
/// describe their files.
///
/// # Errors
///
/// Database errors.
pub(crate) fn bootstrap_files(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO sync_touched (kind, space, id)
         SELECT DISTINCT 'note', n.space_id, n.id FROM notes n JOIN attachments a ON a.note_id = n.id
         WHERE n.space_id NOT IN (SELECT id FROM spaces WHERE role = 'read')",
        [],
    )?;
    let now = crate::clock::Clock::now(&crate::clock::SystemClock);
    flush(conn, &mut Docs::default(), now)
}

/// Documents changed by changes from elsewhere, not rebuilt into rows yet.
/// A page of changes is taken in first, then each document it touched is
/// rebuilt once: a new device taking thousands of small changes to the
/// space would otherwise rebuild after each.
#[derive(Default)]
pub(crate) struct Taken {
    /// Spaces whose document changed, with its state before the first
    /// change.
    spaces: Vec<(String, loro::Frontiers)>,
    /// Notes changed, with their space.
    notes: Vec<(String, NoteId)>,
}

/// Takes a change from another device into its document, see [`rebuild`].
/// Applying the same change twice, or changes in any order, ends the same.
/// A change that builds on one not here yet waits in `sync_waiting` and
/// applies once that one comes.
///
/// # Errors
///
/// [`Error::Invalid`] for an unknown document or bytes that are not a Loro
/// update (nothing is changed then), database errors.
pub(crate) fn import_remote(
    tx: &Connection,
    docs: &mut Docs,
    doc_id: &str,
    bytes: &[u8],
    now: Timestamp,
    taken: &mut Taken,
) -> Result<()> {
    let (space_id, bare) = split_key(doc_id);
    let note = if bare == SPACE {
        None
    } else {
        Some(
            bare.parse::<NoteId>()
                .map_err(|_| Error::Invalid("unknown sync document".into()))?,
        )
    };
    let doc = docs.get(tx, doc_id)?;
    let before = doc.state_frontiers();
    let known = doc.oplog_vv();
    let mine = conflict::Texts {
        title: doc.get_text("title").to_string(),
        body: doc.get_text("body").to_string(),
    };
    let status = doc
        .import(bytes)
        .map_err(|e| Error::Invalid(format!("not a change of this document: {e}")))?;
    if status.pending.is_some() {
        tx.execute(
            "INSERT INTO sync_waiting (doc_id, bytes) VALUES (?1, ?2)",
            params![doc_id, bytes],
        )?;
    }
    if doc.oplog_vv() == known {
        // already had it, or it waits: nothing to rebuild
        return Ok(());
    }
    settle_waiting(tx, &doc, doc_id)?;
    match note {
        Some(note) => {
            check_merge(tx, &doc, note, &known, &status.success, &mine, now)?;
            let entry = (space_id.to_owned(), note);
            if !taken.notes.contains(&entry) {
                taken.notes.push(entry);
            }
        }
        None => {
            if !taken.spaces.iter().any(|(s, _)| s == space_id) {
                taken.spaces.push((space_id.to_owned(), before));
            }
        }
    }
    Ok(())
}

/// Writes the documents changes from elsewhere touched and rebuilds the
/// rows they affect, once each.
///
/// # Errors
///
/// Database errors, a stored document that does not read.
pub(crate) fn rebuild(
    tx: &Connection,
    docs: &mut Docs,
    taken: Taken,
    now: Timestamp,
) -> Result<()> {
    if taken.spaces.is_empty() && taken.notes.is_empty() {
        return Ok(());
    }
    for (space, _) in &taken.spaces {
        docs.persist(tx, &doc_key(space, SPACE), false, now)?;
    }
    for (space, note) in &taken.notes {
        docs.persist(tx, &doc_key(space, &note.to_string()), false, now)?;
    }
    tx.execute("INSERT INTO sync_quiet (on_) VALUES (1)", [])?;
    let rebuilt: Result<()> = (|| {
        let mut done: Vec<(String, NoteId)> = Vec::new();
        let mut whole: Vec<String> = Vec::new();
        for (space_id, before) in &taken.spaces {
            let space = docs.get(tx, &doc_key(space_id, SPACE))?;
            match materialize::touched(&space, before) {
                Some(touched) if !touched.folders => {
                    materialize::some(tx, docs, space_id, &touched, now)?;
                    done.extend(touched.notes.iter().map(|n| (space_id.clone(), *n)));
                }
                // a folder changed: notes may enter or leave the trash with it
                _ => {
                    materialize::all(tx, docs, space_id, now)?;
                    whole.push(space_id.clone());
                }
            }
        }
        for (space_id, note) in taken.notes {
            if !whole.contains(&space_id) && !done.contains(&(space_id.clone(), note)) {
                materialize::note(tx, docs, &space_id, note, now)?;
            }
        }
        Ok(())
    })();
    tx.execute("DELETE FROM sync_quiet", [])?;
    rebuilt
}

/// Takes one change from another device and rebuilds what it touched.
///
/// # Errors
///
/// As [`import_remote`] and [`rebuild`].
pub(crate) fn apply_remote(
    tx: &Connection,
    docs: &mut Docs,
    doc_id: &str,
    bytes: &[u8],
    now: Timestamp,
) -> Result<()> {
    let mut taken = Taken::default();
    import_remote(tx, docs, doc_id, bytes, now, &mut taken)?;
    rebuild(tx, docs, taken, now)
}

/// After a note took in a change from elsewhere: when it was made apart
/// from this device's own edits and both rewrote the same passage, the
/// merge is recorded as a conflict. A change made after a merge, on the
/// other hand, settles a conflict waiting here: someone acted on it.
fn check_merge(
    conn: &Connection,
    doc: &LoroDoc,
    note: NoteId,
    known: &VersionVector,
    imported: &loro::VersionRange,
    mine: &conflict::Texts,
    now: Timestamp,
) -> Result<()> {
    let ends: Vec<loro::ID> = imported
        .iter()
        .filter(|(_, (start, end))| end > start)
        .map(|(peer, (_, end))| loro::ID::new(*peer, end - 1))
        .collect();
    if ends.is_empty() {
        return Ok(());
    }
    let theirs_at = loro::Frontiers::from(ends);
    let Some(theirs_vv) = doc.frontiers_to_vv(&theirs_at) else {
        return Ok(());
    };
    // the base both started from: what each had of the other
    let mut base_vv = VersionVector::new();
    for (peer, counter) in known.iter() {
        let seen = theirs_vv.get(peer).copied().unwrap_or(0).min(*counter);
        if seen > 0 {
            base_vv.insert(*peer, seen);
        }
    }
    if &base_vv == known {
        // the other device had seen all of this one's edits: no merge
        return conflict::settled(conn, note);
    }
    let text_at = |vv: &VersionVector| -> Result<conflict::Texts> {
        let fork = doc.fork_at(&doc.vv_to_frontiers(vv)).map_err(sync_error)?;
        Ok(conflict::Texts {
            title: fork.get_text("title").to_string(),
            body: fork.get_text("body").to_string(),
        })
    };
    let base = text_at(&base_vv)?;
    let theirs = text_at(&theirs_vv)?;
    if conflict::suspicious(&base, mine, &theirs) {
        conflict::record_merge(conn, note, mine, &theirs, now)?;
    }
    Ok(())
}

/// How long the document of a note deleted for good is kept, counted from
/// when the deletion reached this device. An edit made elsewhere before the
/// deletion was seen there brings the note back while it is kept.
pub const FORGET_DELETED_AFTER: Timestamp = 30 * crate::clock::DAY;

/// Notes when a deletion for good reached this device, once. Kept by the
/// note's document key, so the right space forgets it.
pub(crate) fn deletion_arrived(
    conn: &Connection,
    space: &str,
    note: NoteId,
    now: Timestamp,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO trash_arrived (kind, id, at) VALUES ('purged', ?1, ?2)",
        params![doc_key(space, &note.to_string()), now],
    )?;
    Ok(())
}

/// Where a space's sync stands: its cursor on the server.
pub(crate) fn cursor_of(conn: &Connection, space: &str) -> Result<Option<i64>> {
    let sql = if space.is_empty() {
        "SELECT cursor FROM sync_link WHERE id = 1 AND ?1 = ''"
    } else {
        "SELECT cursor FROM spaces WHERE id = ?1"
    };
    Ok(conn.query_row(sql, [space], |row| row.get(0)).optional()?)
}

/// Forgets the documents of notes deleted for good long enough ago: here,
/// and on the server through an empty document that replaces their changes
/// (when linked). A note that came back since keeps its document. Returns
/// how many were forgotten.
///
/// # Errors
///
/// Database errors.
pub(crate) fn forget_deleted(conn: &Connection, docs: &mut Docs, now: Timestamp) -> Result<u32> {
    let due = {
        let mut stmt =
            conn.prepare("SELECT id FROM trash_arrived WHERE kind = 'purged' AND at <= ?1")?;
        stmt.query_map([now - FORGET_DELETED_AFTER], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut forgotten = 0;
    for id in due {
        let (space, note) = split_key(&id);
        let cursor = cursor_of(conn, space)?;
        conn.execute(
            "DELETE FROM trash_arrived WHERE kind = 'purged' AND id = ?1",
            [&id],
        )?;
        let back: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM notes WHERE id = ?1 AND space_id = ?2)",
            params![note, space],
            |row| row.get(0),
        )?;
        let waiting: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sync_outbox WHERE doc_id = ?1)",
            [&id],
            |row| row.get(0),
        )?;
        if back || waiting {
            continue;
        }
        for table in [
            "sync_docs WHERE id",
            "sync_updates WHERE doc_id",
            "sync_waiting WHERE doc_id",
        ] {
            conn.execute(&format!("DELETE FROM {table} = ?1"), [&id])?;
        }
        docs.open.remove(&id);
        if let Some(cursor) = cursor.filter(|&c| c > 0) {
            let empty = LoroDoc::new()
                .export(ExportMode::Snapshot)
                .map_err(sync_error)?;
            conn.execute(
                "INSERT INTO sync_outbox (doc_id, bytes, created_at, compacts) VALUES (?1, ?2, ?3, ?4)",
                params![id, empty, now, cursor],
            )?;
        }
        forgotten += 1;
    }
    Ok(forgotten)
}

/// Drops waiting changes of a document that are now applied.
fn settle_waiting(conn: &Connection, doc: &LoroDoc, doc_id: &str) -> Result<()> {
    let waiting = {
        let mut stmt =
            conn.prepare("SELECT seq, bytes FROM sync_waiting WHERE doc_id = ?1 ORDER BY seq")?;
        stmt.query_map([doc_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (seq, bytes) in waiting {
        // importing again is how Loro says whether it still waits
        let status = doc.import(&bytes).map_err(sync_error)?;
        if status.pending.is_none() {
            conn.execute("DELETE FROM sync_waiting WHERE seq = ?1", [seq])?;
        }
    }
    Ok(())
}

/// Local changes not yet taken by the server, oldest first.
///
/// # Errors
///
/// Database errors.
pub(crate) fn outbox(
    conn: &Connection,
    docs: &mut Docs,
    space: Option<&str>,
    limit: usize,
) -> Result<Vec<Outgoing>> {
    let peer = docs.peer(conn)?;
    // every space, or one: the personal space's keys hold no '/'
    let mut stmt = conn.prepare(
        "SELECT seq, doc_id, bytes, compacts FROM sync_outbox
         WHERE ?2 IS NULL OR (?2 = '' AND instr(doc_id, '/') = 0) OR doc_id LIKE ?2 || '/%'
         ORDER BY seq LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(
            params![i64::try_from(limit).unwrap_or(i64::MAX), space],
            |row| {
                let seq: i64 = row.get(0)?;
                Ok(Outgoing {
                    seq,
                    id: format!("{peer:x}-{seq}"),
                    doc_id: row.get(1)?,
                    bytes: row.get(2)?,
                    compacts: row.get(3)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Replaces the outbox with one change per document holding all of it,
/// for an account that has seen nothing of this vault. Cheaper to send and
/// to take in than every edit since the vault began.
///
/// # Errors
///
/// Database errors, a stored document that does not read.
pub(crate) fn requeue_all(
    conn: &Connection,
    docs: &mut Docs,
    space: &str,
    now: Timestamp,
) -> Result<()> {
    let in_space = "(?1 = '' AND instr(k, '/') = 0) OR k LIKE ?1 || '/%'";
    let ids = {
        let mut stmt = conn.prepare(&format!(
            "SELECT k FROM (SELECT id AS k FROM sync_docs UNION SELECT doc_id FROM sync_updates)
             WHERE {in_space} ORDER BY 1"
        ))?;
        stmt.query_map([space], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    conn.execute(
        &format!(
            "DELETE FROM sync_outbox WHERE seq IN
                 (SELECT seq FROM (SELECT seq, doc_id AS k FROM sync_outbox) WHERE {in_space})"
        ),
        [space],
    )?;
    // the space first: a device taking the notes in already knows where
    // they sit
    let space_doc = doc_key(space, SPACE);
    for id in
        std::iter::once(space_doc.clone()).chain(ids.into_iter().filter(|id| *id != space_doc))
    {
        if !docs.stored(conn, &id)? {
            continue;
        }
        let bytes = snapshot(conn, docs, &id)?;
        conn.execute(
            "INSERT INTO sync_outbox (doc_id, bytes, created_at) VALUES (?1, ?2, ?3)",
            params![id, bytes, now],
        )?;
    }
    Ok(())
}

/// Drops outbox entries the server has taken.
///
/// # Errors
///
/// Database errors.
pub(crate) fn ack(conn: &Connection, space: Option<&str>, up_to: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM sync_outbox WHERE seq <= ?1
             AND (?2 IS NULL OR (?2 = '' AND instr(doc_id, '/') = 0) OR doc_id LIKE ?2 || '/%')",
        params![up_to, space],
    )?;
    Ok(())
}

/// How much sync holds: documents, and changes waiting for the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncCounts {
    /// Documents stored.
    pub documents: i64,
    /// Changes not yet taken by the server.
    pub pending: i64,
    /// Their size in bytes.
    pub pending_bytes: i64,
}

/// Counts for status lines and dev tools.
///
/// # Errors
///
/// Database errors.
pub(crate) fn counts(conn: &Connection) -> Result<SyncCounts> {
    Ok(conn.query_row(
        "SELECT (SELECT count(*) FROM (SELECT id FROM sync_docs UNION SELECT doc_id FROM sync_updates)),
                (SELECT count(*) FROM sync_outbox),
                (SELECT coalesce(sum(length(bytes)), 0) FROM sync_outbox)",
        [],
        |row| {
            Ok(SyncCounts {
                documents: row.get(0)?,
                pending: row.get(1)?,
                pending_bytes: row.get(2)?,
            })
        },
    )?)
}

/// The whole state of a document, for a device that has nothing yet.
///
/// # Errors
///
/// Database errors, a stored document that does not read.
pub(crate) fn snapshot(conn: &Connection, docs: &mut Docs, doc_id: &str) -> Result<Vec<u8>> {
    let doc = docs.get(conn, doc_id)?;
    doc.export(ExportMode::Snapshot).map_err(sync_error)
}
