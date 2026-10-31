// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared spaces in this vault: joining them as the server lists them,
//! leaving them (every trace of one goes), their keys, and moving notes and
//! folders between spaces.
//!
//! A move copies: the items get new ids in the space they go to, with their
//! text and its history, tags, pin, dates, attachments and versions; the
//! originals are deleted for good in the space they leave. An edit made
//! elsewhere before the move was seen there brings an original back, by the
//! usual rule for an edit after a deletion. A note never lives in two
//! spaces, and nothing typed is lost.

use std::collections::HashMap;

use app_protocol::e2ee::AccountKind;
use app_protocol::sync::{
    GiveGrants, GrantGiven, GrantWanted, SharedSpace as Listed, SpaceGrant, SpaceRole,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::{Store, load_note, reindex};
use crate::error::{Error, Result};
use crate::id::{AttachmentId, FolderId, NoteId};
use crate::sync::{SPACE, doc_key};

/// A member of an e2ee space waiting for the owner to let them in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitingMember {
    /// Their account.
    pub user_id: String,
    /// Their username, as the server says.
    pub name: String,
    /// The key the server shows for them now.
    pub public_key: String,
    /// The owner let them in with another key: someone may stand in for
    /// them, compare fingerprints before letting them in again.
    pub changed: bool,
}

/// A shared space as this vault knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSpace {
    /// Its id.
    pub id: String,
    /// `owner`, `edit` or `read`.
    pub role: String,
    /// `standard` or `e2ee`.
    pub kind: String,
    /// Who shared it.
    pub owner_name: String,
}

/// What a shared space holds at its top: the folders and notes the sidebar
/// shows under "Shared".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceRoots {
    /// Folders with no parent, not in the trash.
    pub folders: Vec<FolderId>,
    /// Notes in no folder, not in the trash, with their titles.
    pub notes: Vec<RootNote>,
}

/// A note at the top of a shared space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootNote {
    /// The note.
    pub id: NoteId,
    /// Its title.
    pub title: String,
}

/// What to move into another space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// One note.
    Note(NoteId),
    /// A folder, everything below it and their notes.
    Folder(FolderId),
}

fn role_text(role: SpaceRole) -> &'static str {
    match role {
        SpaceRole::Owner => "owner",
        SpaceRole::Edit => "edit",
        SpaceRole::Read => "read",
    }
}

fn kind_text(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::Standard => "standard",
        AccountKind::E2ee => "e2ee",
    }
}

/// Removes every trace of a space from the vault, quietly: its rows go
/// without being recorded as deletions, its documents and queued changes
/// too.
pub(super) fn drop_space(conn: &Connection, space: &str) -> Result<()> {
    conn.execute("INSERT INTO sync_quiet (on_) VALUES (1)", [])?;
    let dropped: Result<()> = (|| {
        conn.execute(
            "DELETE FROM notes_fts WHERE rowid IN (SELECT rowid FROM notes WHERE space_id = ?1)",
            [space],
        )?;
        conn.execute("DELETE FROM notes WHERE space_id = ?1", [space])?;
        conn.execute("DELETE FROM folders WHERE space_id = ?1", [space])?;
        conn.execute(
            "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM note_tags)",
            [],
        )?;
        let prefix = format!("{space}/%");
        for table in [
            "sync_docs WHERE id",
            "sync_updates WHERE doc_id",
            "sync_outbox WHERE doc_id",
            "sync_waiting WHERE doc_id",
            "sync_rejected WHERE doc_id",
            "trash_arrived WHERE id",
        ] {
            conn.execute(&format!("DELETE FROM {table} LIKE ?1"), [&prefix])?;
        }
        conn.execute("DELETE FROM spaces WHERE id = ?1", [space])?;
        conn.execute("DELETE FROM files_on_server WHERE space = ?1", [space])?;
        // if it comes back, this device writes it under a new peer id: its
        // earlier changes are no longer here to number new ones after
        conn.execute(
            "INSERT OR REPLACE INTO vault_meta (key, value) VALUES (?1, ?2)",
            params![
                format!("peer:{space}"),
                (u64::from_le_bytes(crate::crypto::random_bytes::<8>()?) >> 1)
                    .to_le_bytes()
                    .to_vec()
            ],
        )?;
        // if it comes back, this device's own changes on the server are
        // no longer here: they are taken in again with the others
        conn.execute(
            "INSERT OR REPLACE INTO vault_meta (key, value) VALUES (?1, x'01')",
            [format!("rejoin:{space}")],
        )?;
        Ok(())
    })();
    conn.execute("DELETE FROM sync_quiet", [])?;
    dropped
}

/// Before a space goes from this vault: its notes with edits not sent yet
/// are copied into the personal space, at its top, so what was written
/// here is never lost with the space (a member removed, or made a reader,
/// while offline). Nothing is recorded in the space itself. Returns how
/// many notes were kept.
fn salvage(conn: &Connection, docs: &mut crate::sync::Docs, space: &str) -> Result<u32> {
    let pending: Vec<NoteId> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM notes WHERE space_id = ?1
               AND EXISTS (SELECT 1 FROM sync_outbox o WHERE o.doc_id = ?1 || '/' || notes.id)",
        )?;
        stmt.query_map([space], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for old in &pending {
        copy_note(conn, docs, space, "", *old, &HashMap::new())?;
    }
    // the copies are recorded in the personal space, the space records nothing
    conn.execute("DELETE FROM sync_touched WHERE space = ?1", [space])?;
    Ok(u32::try_from(pending.len()).unwrap_or(u32::MAX))
}

/// Copies folders into a space with new ids, the moved tree's top at the
/// top of the space. Returns the new id of each.
fn copy_folders(
    conn: &Connection,
    folders: &[FolderId],
    space: &str,
) -> Result<HashMap<FolderId, FolderId>> {
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
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![new, parent, name, position, created, updated, trashed, space],
        )?;
    }
    Ok(new_folders)
}

/// Copies a note into a space with a new id: its text with the text's
/// history, tags, pin, dates, versions and attachments. The original row
/// goes, which records its deletion in the space it left.
fn copy_note(
    conn: &Connection,
    docs: &mut crate::sync::Docs,
    source: &str,
    space: &str,
    old: NoteId,
    new_folders: &HashMap<FolderId, FolderId>,
) -> Result<NoteId> {
    let new = NoteId::new();
    let note = load_note(conn, old)?;
    let from = docs.get(conn, &doc_key(source, &old.to_string()))?;
    let snapshot = from
        .export(loro::ExportMode::Snapshot)
        .map_err(|e| Error::Corrupt(format!("sync document: {e}")))?;
    let to = docs.get(conn, &doc_key(space, &new.to_string()))?;
    to.import(&snapshot)
        .map_err(|e| Error::Corrupt(format!("sync document: {e}")))?;
    let folder = note.folder_id.and_then(|f| new_folders.get(&f).copied());
    conn.execute(
        "INSERT INTO notes (id, folder_id, title, body, pinned, created_at, updated_at, trashed_at, space_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            new,
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
    conn.execute(
        "INSERT INTO note_tags (note_id, tag_id) SELECT ?2, tag_id FROM note_tags WHERE note_id = ?1",
        params![old, new],
    )?;
    conn.execute(
        "UPDATE note_versions SET note_id = ?2 WHERE note_id = ?1",
        params![old, new],
    )?;
    // attachments get new ids too: another device may hold the original
    // note and the copy at once while the move reaches it
    let files: Vec<AttachmentId> = {
        let mut stmt = conn.prepare("SELECT id FROM attachments WHERE note_id = ?1")?;
        stmt.query_map([old], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    // where the files were, for a device that has not fetched them yet
    conn.execute(
        "INSERT OR IGNORE INTO file_sources (sha256, space, epoch)
         SELECT sha256, ?2, epoch FROM attachments WHERE note_id = ?1
         UNION SELECT thumb_sha256, ?2, epoch FROM attachments
         WHERE note_id = ?1 AND thumb_sha256 IS NOT NULL",
        params![old, source],
    )?;
    // the epoch belonged to the space the files leave
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

impl Store {
    /// The shared spaces this vault holds.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn shared_spaces(&self) -> Result<Vec<LocalSpace>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, role, kind, owner_name FROM spaces ORDER BY id")?;
        let spaces = stmt
            .query_map([], |row| {
                Ok(LocalSpace {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    kind: row.get(2)?,
                    owner_name: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(spaces)
    }

    /// Follows the server's list of the shared spaces this account belongs
    /// to: new ones are added (their notes come with the next pull), roles
    /// follow, spaces no longer listed are dropped with everything in them.
    /// Returns the ids dropped.
    ///
    /// # Errors
    ///
    /// Database errors, a stored document that does not read.
    pub fn sync_set_spaces(&mut self, listed: &[Listed]) -> Result<Vec<String>> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        for space in listed {
            tx.execute(
                // a space seen e2ee stays e2ee here, whatever the server says
                // later: its word alone never turns encryption off
                "INSERT INTO spaces (id, role, kind, owner_name) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (id) DO UPDATE SET role = excluded.role,
                     kind = CASE WHEN spaces.kind = 'e2ee' THEN 'e2ee' ELSE excluded.kind END,
                     owner_name = excluded.owner_name",
                params![
                    space.id,
                    role_text(space.role),
                    kind_text(space.kind),
                    space.owner
                ],
            )?;
        }
        let here = {
            let mut stmt = tx.prepare("SELECT id FROM spaces")?;
            stmt.query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut dropped = Vec::new();
        for id in here {
            if !listed.iter().any(|s| s.id == id) {
                salvage(&tx, &mut self.docs, &id)?;
                drop_space(&tx, &id)?;
                dropped.push(id);
            }
        }
        crate::sync::finish(tx, &mut self.docs, now)?;
        if !dropped.is_empty() {
            self.docs.forget();
        }
        Ok(dropped)
    }

    /// Whether this vault ever signed in to `username` on `server` as an
    /// e2ee account. Kept past signing out: a server that later calls the
    /// account standard is not believed.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn e2ee_seen(&self, server: &str, username: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM vault_meta WHERE key = ?1)",
            [e2ee_key(server, username)],
            |row| row.get(0),
        )?)
    }

    /// Remembers that `username` on `server` is an e2ee account.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn remember_e2ee(&mut self, server: &str, username: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO vault_meta (key, value) VALUES (?1, x'01')",
            [e2ee_key(server, username)],
        )?;
        Ok(())
    }

    /// Leaves a shared space here: everything of it goes from this vault.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn leave_space(&mut self, space: &str) -> Result<()> {
        if space.is_empty() {
            return Err(Error::Invalid("the personal space cannot be left".into()));
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        salvage(&tx, &mut self.docs, space)?;
        drop_space(&tx, space)?;
        crate::sync::finish(tx, &mut self.docs, now)?;
        self.docs.forget();
        Ok(())
    }

    /// The server refused what this device sent to a shared space (made a
    /// reader, removed, the space gone): notes with unsent edits are kept
    /// as personal copies, the space goes from here and, if this account is
    /// still in it, comes back whole at the next round. Returns how many
    /// notes were kept.
    ///
    /// # Errors
    ///
    /// Database errors, a stored document that does not read.
    pub fn sync_lose_space(&mut self, space: &str) -> Result<u32> {
        if space.is_empty() {
            return Err(Error::Invalid("the personal space cannot be lost".into()));
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        let kept = salvage(&tx, &mut self.docs, space)?;
        drop_space(&tx, space)?;
        crate::sync::finish(tx, &mut self.docs, now)?;
        self.docs.forget();
        Ok(kept)
    }

    /// Keeps a key of an e2ee shared space.
    ///
    /// # Errors
    ///
    /// Database errors, a space this vault does not hold.
    pub fn set_space_key(&mut self, space: &str, epoch: u32, key: &[u8; 32]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO space_keys (space_id, epoch, key) VALUES (?1, ?2, ?3)",
            params![space, epoch, key.as_slice()],
        )?;
        Ok(())
    }

    /// The epochs of a space this vault has keys for.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn space_key_epochs(&self, space: &str) -> Result<Vec<u32>> {
        let mut stmt = self
            .conn
            .prepare("SELECT epoch FROM space_keys WHERE space_id = ?1 ORDER BY epoch")?;
        let epochs = stmt
            .query_map([space], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<u32>>>()?;
        Ok(epochs)
    }

    /// Keeps this account's key pair for sharing (e2ee accounts).
    ///
    /// # Errors
    ///
    /// Database errors, a vault that is not linked.
    pub fn sync_set_key_pair(&mut self, pair: &app_e2ee::KeyPair) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE sync_link SET key_public = ?1, key_private = ?2 WHERE id = 1",
            params![pair.public_text(), pair.private().as_slice()],
        )?;
        if changed == 0 {
            return Err(Error::Invalid(
                "this vault is not linked to an account".into(),
            ));
        }
        Ok(())
    }

    /// This account's key pair, if it has one here.
    ///
    /// # Errors
    ///
    /// Database errors, a stored pair that does not hold together.
    pub fn sync_key_pair(&self) -> Result<Option<app_e2ee::KeyPair>> {
        let parts: Option<(Option<String>, Option<Vec<u8>>)> = self
            .conn
            .query_row(
                "SELECT key_public, key_private FROM sync_link WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((Some(public), Some(private))) = parts else {
            return Ok(None);
        };
        let private: [u8; 32] = private
            .as_slice()
            .try_into()
            .map_err(|_| Error::Corrupt("key pair".into()))?;
        app_e2ee::KeyPair::from_parts(&public, &private)
            .map(Some)
            .map_err(|_| Error::Corrupt("key pair".into()))
    }

    /// Opens the space keys the server holds for this account and keeps
    /// the ones not here yet. Returns how many were new.
    ///
    /// # Errors
    ///
    /// Database errors. A grant that does not open (made for another key)
    /// is skipped: it changes nothing here.
    pub fn sync_accept_grants(&mut self, listed: &Listed) -> Result<u32> {
        let space = listed.id.as_str();
        let Some(pair) = self.sync_key_pair()? else {
            return Ok(0);
        };
        let owner = self.space_owner_key(listed, &pair)?;
        let mut known = self.space_key_epochs(space)?;
        // a device with no key of the space yet trusts the server's list as
        // it is, grants from before owners sealed them included; after that
        // only the owner's, one epoch after the other
        let fresh = known.is_empty();
        let mut grants: Vec<&SpaceGrant> = listed.grants.iter().collect();
        grants.sort_by_key(|g| g.epoch);
        let mut new = 0;
        for grant in grants {
            if known.contains(&grant.epoch) {
                continue;
            }
            let next = known.iter().max().map_or(u32::MAX, |m| m.saturating_add(1));
            if !fresh && grant.epoch > next {
                continue;
            }
            let signed = owner
                .as_deref()
                .and_then(|o| app_e2ee::accept(space, grant.epoch, &grant.grant, &pair, o).ok());
            let key = signed.or_else(|| {
                fresh
                    .then(|| {
                        app_e2ee::accept_unsigned(space, grant.epoch, &grant.grant, &pair).ok()
                    })
                    .flatten()
            });
            if let Some(key) = key {
                self.set_space_key(space, grant.epoch, &key)?;
                known.push(grant.epoch);
                new += 1;
            }
        }
        if new > 0 {
            // changes sealed with a key that had not come yet were set
            // aside: the space is read again from its start, they open now
            // and the rest changes nothing taken in twice
            let prefix = format!("{space}/%");
            let set_aside = self
                .conn
                .execute("DELETE FROM sync_rejected WHERE doc_id LIKE ?1", [&prefix])?;
            if set_aside > 0 {
                self.conn.execute(
                    "UPDATE spaces SET cursor = 0, cursor_id = NULL WHERE id = ?1",
                    [space],
                )?;
            }
        }
        Ok(new)
    }

    /// The public key grants of this space must come from: this account's
    /// own for a space it owns, else the owner's key as first listed here,
    /// kept from then on. A server listing another key later is not
    /// believed. `None` while no server listed one.
    fn space_owner_key(
        &mut self,
        listed: &Listed,
        pair: &app_e2ee::KeyPair,
    ) -> Result<Option<String>> {
        if listed.role == SpaceRole::Owner {
            return Ok(Some(pair.public_text()));
        }
        let key = format!("owner_key:{}", listed.id);
        let pinned: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT value FROM vault_meta WHERE key = ?1",
                [&key],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(pinned) = pinned {
            return Ok(Some(String::from_utf8_lossy(&pinned).into_owned()));
        }
        if !app_e2ee::public_key_ok(&listed.owner_key) {
            return Ok(None);
        }
        self.conn.execute(
            "INSERT INTO vault_meta (key, value) VALUES (?1, ?2)",
            params![key, listed.owner_key.as_bytes()],
        )?;
        Ok(Some(listed.owner_key.clone()))
    }

    /// Keys this owner's device should seal for members of an e2ee space
    /// it owns: those the server lists as waiting, epoch by epoch, with the
    /// keys this vault holds. A first key for the latest epoch, when nobody
    /// made it yet, is made here; it is only used to seal changes once the
    /// server hands it back as this account's own grant, so two devices of
    /// the owner can never seal with different keys.
    ///
    /// Only for this account's own key and for members the owner let in
    /// with the key they had then (`space_trust`): a server that lists
    /// someone else, or swaps a member's key, gets nothing. Those wait in
    /// `space_waiting` for the owner to look at them.
    ///
    /// # Errors
    ///
    /// Database errors, a public key that is not one.
    pub fn sync_grants_to_give(&mut self, listed: &Listed) -> Result<Vec<GiveGrants>> {
        if listed.role != SpaceRole::Owner || listed.kind != AccountKind::E2ee {
            return Ok(Vec::new());
        }
        let Some(pair) = self.sync_key_pair()? else {
            return Ok(Vec::new());
        };
        let wanted = self.trusted_wanted(listed)?;
        let latest = listed.epoch.max(1);
        let mut epochs: Vec<u32> = wanted.iter().map(|w| w.epoch).collect();
        epochs.sort_unstable();
        epochs.dedup();
        let mut out = Vec::new();
        for epoch in epochs {
            let stored: Option<Vec<u8>> = self
                .conn
                .query_row(
                    "SELECT key FROM space_keys WHERE space_id = ?1 AND epoch = ?2",
                    params![listed.id, epoch],
                    |row| row.get(0),
                )
                .optional()?;
            let key = match stored {
                Some(bytes) => zeroize::Zeroizing::new(
                    <[u8; 32]>::try_from(bytes.as_slice())
                        .map_err(|_| Error::Corrupt("space key".into()))?,
                ),
                // a key nobody of this account made yet, for the latest epoch
                None if epoch == latest && !listed.grants.iter().any(|g| g.epoch == epoch) => {
                    app_e2ee::new_space_key()
                }
                // made elsewhere: its grant comes, or an older key this
                // device never had
                None => continue,
            };
            let mut grants = Vec::new();
            for wanted in wanted.iter().filter(|w| w.epoch == epoch) {
                let grant = app_e2ee::grant(&listed.id, epoch, &key, &wanted.public_key, &pair)
                    .map_err(|_| Error::Invalid("a member's public key is not one".into()))?;
                grants.push(GrantGiven {
                    user_id: wanted.user_id.clone(),
                    grant,
                });
            }
            out.push(GiveGrants { epoch, grants });
        }
        Ok(out)
    }

    /// The waiting members the owner let in, with the key they had then.
    /// A person let in before they had a key keeps the first one seen. The
    /// rest go to `space_waiting`, which this replaces for the space.
    fn trusted_wanted<'a>(&mut self, listed: &'a Listed) -> Result<Vec<&'a GrantWanted>> {
        let own = self.sync_key_pair()?.map(|pair| pair.public_text());
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM space_waiting WHERE space_id = ?1",
            [&listed.id],
        )?;
        let mut out = Vec::new();
        for wanted in &listed.grants_wanted {
            // this account's own grant, which every device of it opens
            if own.as_deref() == Some(wanted.public_key.as_str()) {
                out.push(wanted);
                continue;
            }
            let kept: Option<Option<String>> = tx
                .query_row(
                    "SELECT public_key FROM space_trust WHERE space_id = ?1 AND user_id = ?2",
                    params![listed.id, wanted.user_id],
                    |row| row.get(0),
                )
                .optional()?;
            match kept {
                Some(Some(key)) if key == wanted.public_key => out.push(wanted),
                Some(None) => {
                    tx.execute(
                        "UPDATE space_trust SET public_key = ?3 WHERE space_id = ?1 AND user_id = ?2",
                        params![listed.id, wanted.user_id, wanted.public_key],
                    )?;
                    out.push(wanted);
                }
                kept => {
                    tx.execute(
                        "INSERT INTO space_waiting (space_id, user_id, name, public_key, changed)
                         VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT (space_id, user_id) DO UPDATE SET
                           name = excluded.name, public_key = excluded.public_key,
                           changed = excluded.changed",
                        params![
                            listed.id,
                            wanted.user_id,
                            wanted.name,
                            wanted.public_key,
                            kept.is_some()
                        ],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(out)
    }

    /// Lets a member of an e2ee space this account owns have its keys, with
    /// this public key, `None` for the first one they will have. Their keys
    /// are sealed at the next sync round.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn space_trust(
        &mut self,
        space: &str,
        user_id: &str,
        public_key: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO space_trust (space_id, user_id, public_key) VALUES (?1, ?2, ?3)
             ON CONFLICT (space_id, user_id) DO UPDATE SET public_key = excluded.public_key",
            params![space, user_id, public_key],
        )?;
        self.conn.execute(
            "DELETE FROM space_waiting WHERE space_id = ?1 AND user_id = ?2",
            params![space, user_id],
        )?;
        Ok(())
    }

    /// Whether the owner let this member in already, whatever key.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn space_trusted(&self, space: &str, user_id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM space_trust WHERE space_id = ?1 AND user_id = ?2)",
            params![space, user_id],
            |row| row.get(0),
        )?)
    }

    /// Members of a space the owner has not let in, or whose key changed.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn space_waiting(&self, space: &str) -> Result<Vec<WaitingMember>> {
        let mut stmt = self.conn.prepare(
            "SELECT user_id, name, public_key, changed FROM space_waiting
             WHERE space_id = ?1 ORDER BY name",
        )?;
        let rows = stmt
            .query_map([space], |row| {
                Ok(WaitingMember {
                    user_id: row.get(0)?,
                    name: row.get(1)?,
                    public_key: row.get(2)?,
                    changed: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The folders and notes at the top of a space.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn space_roots(&self, space: &str) -> Result<SpaceRoots> {
        let mut folders = self.conn.prepare(
            "SELECT id FROM folders WHERE space_id = ?1 AND parent_id IS NULL AND trashed_at IS NULL
             ORDER BY position, name",
        )?;
        let folders = folders
            .query_map([space], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|id| id.parse().ok())
            .collect();
        let mut notes = self.conn.prepare(
            "SELECT id, title FROM notes WHERE space_id = ?1 AND folder_id IS NULL AND trashed_at IS NULL
             ORDER BY title",
        )?;
        let notes = notes
            .query_map([space], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(id, title)| {
                Some(RootNote {
                    id: id.parse().ok()?,
                    title,
                })
            })
            .collect();
        Ok(SpaceRoots { folders, notes })
    }

    /// This account's master key, e2ee accounts only: the device seals the
    /// account's key pair with it.
    ///
    /// # Errors
    ///
    /// Database errors, a stored key that does not read.
    pub fn sync_master_key(&self) -> Result<Option<app_e2ee::MasterKey>> {
        let key: Option<Option<Vec<u8>>> = self
            .conn
            .query_row("SELECT master_key FROM sync_link WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        key.flatten()
            .map(|bytes| {
                app_e2ee::MasterKey::from_slice(&bytes)
                    .map_err(|_| Error::Corrupt("account key".into()))
            })
            .transpose()
    }

    /// Whether a shared space can be synced here: always for a standard
    /// one, once this vault holds a key for an e2ee one.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn space_ready(&self, space: &str) -> Result<bool> {
        if space.is_empty() {
            return Ok(true);
        }
        Ok(self.conn.query_row(
            "SELECT kind != 'e2ee' OR EXISTS (SELECT 1 FROM space_keys k WHERE k.space_id = s.id)
             FROM spaces s WHERE s.id = ?1",
            [space],
            |row| row.get(0),
        )?)
    }

    /// Moves a note, or a folder with everything below it, into another
    /// space (`""` is the personal space), at its top level. Returns the
    /// new id of what was moved.
    ///
    /// # Errors
    ///
    /// An item that does not exist, a space this vault does not hold, a
    /// space shared with this account to read only (either side), database
    /// errors.
    pub fn move_to_space(&mut self, item: Item, space: &str) -> Result<String> {
        if !space.is_empty() {
            let known: bool = self.conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM spaces WHERE id = ?1)",
                [space],
                |row| row.get(0),
            )?;
            if !known {
                return Err(Error::NotFound("space"));
            }
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        let (folders, notes) = match item {
            Item::Note(id) => (Vec::new(), vec![load_note(&tx, id)?.id]),
            Item::Folder(id) => {
                let folders = super::folders::subtree(&tx, id)?;
                let mut notes = Vec::new();
                for folder in &folders {
                    let mut stmt = tx.prepare("SELECT id FROM notes WHERE folder_id = ?1")?;
                    notes.extend(
                        stmt.query_map([folder], |row| row.get::<_, NoteId>(0))?
                            .collect::<rusqlite::Result<Vec<_>>>()?,
                    );
                }
                (folders, notes)
            }
        };
        let source: String = match item {
            Item::Note(id) => load_note(&tx, id)?.space_id,
            Item::Folder(id) => tx
                .query_row("SELECT space_id FROM folders WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .optional()?
                .ok_or(Error::NotFound("folder"))?,
        };
        if source == space {
            return Err(Error::Invalid("already in that space".into()));
        }
        let new_folders = copy_folders(&tx, &folders, space)?;
        let mut moved_id = match item {
            Item::Folder(id) => new_folders[&id].to_string(),
            Item::Note(_) => String::new(),
        };
        for old in &notes {
            let new = copy_note(&tx, &mut self.docs, &source, space, *old, &new_folders)?;
            if moved_id.is_empty() {
                moved_id = new.to_string();
            }
        }
        // children first, a folder row refers to its parent
        for old in folders.iter().rev() {
            tx.execute("DELETE FROM folders WHERE id = ?1", [old])?;
        }
        // the space document of a space new here starts with this change
        let _ = self.docs.get(&tx, &doc_key(space, SPACE))?;
        crate::sync::finish(tx, &mut self.docs, now)?;
        Ok(moved_id)
    }
}

/// The `vault_meta` key for an e2ee account: the server without a trailing
/// slash, the username lowercased as servers compare them.
fn e2ee_key(server: &str, username: &str) -> String {
    format!(
        "e2ee:{}|{}",
        server.trim().trim_end_matches('/').to_ascii_lowercase(),
        username.trim().to_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use app_protocol::e2ee::AccountKind;
    use app_protocol::sync::{SharedSpace, SpaceRole};

    use crate::store::test_support::store;

    fn listed(kind: AccountKind) -> SharedSpace {
        SharedSpace {
            id: "s1".into(),
            role: SpaceRole::Edit,
            kind,
            owner: "alex".into(),
            owner_key: String::new(),
            head: 0,
            epoch: 0,
            grants: Vec::new(),
            grants_wanted: Vec::new(),
        }
    }

    #[test]
    fn a_space_seen_e2ee_stays_e2ee() {
        let (mut store, _) = store();
        store.sync_set_spaces(&[listed(AccountKind::E2ee)]).unwrap();
        store
            .sync_set_spaces(&[listed(AccountKind::Standard)])
            .unwrap();
        assert_eq!(store.shared_spaces().unwrap()[0].kind, "e2ee");
    }

    #[test]
    fn a_standard_space_may_become_e2ee() {
        let (mut store, _) = store();
        store
            .sync_set_spaces(&[listed(AccountKind::Standard)])
            .unwrap();
        store.sync_set_spaces(&[listed(AccountKind::E2ee)]).unwrap();
        assert_eq!(store.shared_spaces().unwrap()[0].kind, "e2ee");
    }

    #[test]
    fn e2ee_accounts_are_remembered_by_server_and_username() {
        let (mut store, _) = store();
        assert!(!store.e2ee_seen("https://notes.example", "Alex").unwrap());
        store
            .remember_e2ee("https://Notes.example/", "Alex")
            .unwrap();
        assert!(store.e2ee_seen("https://notes.example", "alex").unwrap());
        assert!(!store.e2ee_seen("https://other.example", "alex").unwrap());
        assert!(!store.e2ee_seen("https://notes.example", "sam").unwrap());
    }
}
