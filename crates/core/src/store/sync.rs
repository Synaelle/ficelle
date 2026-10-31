// SPDX-License-Identifier: AGPL-3.0-or-later

//! The vault's link to a server account, and the two halves of a sync
//! round that touch the database: a batch to push, a page to take in. The
//! network sits between them in [`crate::sync::engine`], with no lock held.

use app_e2ee::MasterKey;
use app_protocol::e2ee::AccountKind;
use app_protocol::sync::{OutgoingChange, PullAnswer, PushRequest};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rusqlite::{OptionalExtension, params};
use zeroize::Zeroizing;

use super::Store;
use crate::error::{Error, Result};
use crate::sync::SPACE;

/// The tokens of a signed in device. Debug output never shows them.
#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    /// Sent with every request.
    pub access: String,
    /// When it stops working, milliseconds.
    pub access_expires_at: i64,
    /// Trades for a new pair.
    pub refresh: String,
    /// When it stops working, milliseconds.
    pub refresh_expires_at: i64,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access_expires_at", &self.access_expires_at)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish_non_exhaustive()
    }
}

/// A new link, from a sign-in.
pub struct NewLink {
    /// Server address, as typed and checked.
    pub server: String,
    /// Account id on that server.
    pub account_id: String,
    /// Name to show.
    pub username: String,
    /// Who encrypts.
    pub kind: AccountKind,
    /// E2ee: the account master key.
    pub master_key: Option<MasterKey>,
    /// This device's id on the server.
    pub device_id: String,
    /// The session.
    pub tokens: Tokens,
}

/// The link as stored, without the account key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncLink {
    /// Server address.
    pub server: String,
    /// Account id on that server.
    pub account_id: String,
    /// Name to show.
    pub username: String,
    /// Who encrypts.
    pub kind: AccountKind,
    /// This device's id on the server.
    pub device_id: String,
    /// `None` once signed out: the link stays, sync waits for a sign-in.
    pub tokens: Option<Tokens>,
    /// Number of the last change taken in.
    pub cursor: i64,
    /// Its name, to check the server still has it under that number.
    pub cursor_id: Option<String>,
    /// When the last round ended well.
    pub last_sync_at: Option<i64>,
    /// Why the last round failed, cleared by a good one.
    pub last_error: Option<String>,
}

/// What taking in a page did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Taken {
    /// Changes applied.
    pub applied: u32,
    /// This device's own changes, already here.
    pub own: u32,
    /// Changes that could not be read, set aside.
    pub rejected: u32,
}

/// Cipher of a link: the server encrypts for standard accounts, the device
/// for e2ee ones.
enum Cipher {
    Standard,
    E2ee(MasterKey),
    /// An e2ee shared space: its keys by epoch, the newest seals. A sealed
    /// change starts with the epoch it was sealed in.
    Shared(Vec<(u32, Zeroizing<[u8; 32]>)>),
}

impl Cipher {
    fn seal(&self, doc: &str, bytes: &[u8]) -> Result<Vec<u8>> {
        Ok(match self {
            Self::Standard => bytes.to_vec(),
            Self::E2ee(master) => {
                app_e2ee::seal_change(&app_e2ee::doc_key(master, doc), doc, bytes)
            }
            Self::Shared(keys) => {
                let (epoch, key) = keys
                    .last()
                    .ok_or_else(|| Error::Invalid("no key for this shared space yet".into()))?;
                let sealed = app_e2ee::seal_change(&app_e2ee::space_doc_key(key, doc), doc, bytes);
                [epoch.to_be_bytes().as_slice(), &sealed].concat()
            }
        })
    }

    fn open(&self, doc: &str, bytes: Vec<u8>) -> std::result::Result<Zeroizing<Vec<u8>>, String> {
        match self {
            Self::Standard => Ok(Zeroizing::new(bytes)),
            Self::E2ee(master) => {
                app_e2ee::open_change(&app_e2ee::doc_key(master, doc), doc, &bytes)
                    .map_err(|e| e.to_string())
            }
            Self::Shared(keys) => {
                let (epoch, sealed) = bytes
                    .split_at_checked(4)
                    .ok_or_else(|| "too short".to_owned())?;
                let epoch = u32::from_be_bytes(epoch.try_into().map_err(|_| "epoch")?);
                let (_, key) = keys
                    .iter()
                    .find(|(e, _)| *e == epoch)
                    .ok_or_else(|| format!("no key for epoch {epoch}"))?;
                app_e2ee::open_change(&app_e2ee::space_doc_key(key, doc), doc, sealed)
                    .map_err(|e| e.to_string())
            }
        }
    }
}

fn kind_text(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::Standard => "standard",
        AccountKind::E2ee => "e2ee",
    }
}

fn link_key(server: &str, account: &str) -> String {
    format!("{server}\n{account}")
}

impl Store {
    /// The server account this vault syncs with.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_link(&self) -> Result<Option<SyncLink>> {
        Ok(self
            .conn
            .query_row(
                "SELECT server, account_id, username, kind, device_id, access_token,
                        access_expires_at, refresh_token, refresh_expires_at, cursor,
                        last_sync_at, last_error, cursor_id
                 FROM sync_link WHERE id = 1",
                [],
                |row| {
                    let access: Option<String> = row.get(5)?;
                    let refresh: Option<String> = row.get(7)?;
                    let tokens = match (access, refresh) {
                        (Some(access), Some(refresh)) => Some(Tokens {
                            access,
                            access_expires_at: row.get(6)?,
                            refresh,
                            refresh_expires_at: row.get(8)?,
                        }),
                        _ => None,
                    };
                    Ok(SyncLink {
                        server: row.get(0)?,
                        account_id: row.get(1)?,
                        username: row.get(2)?,
                        kind: if row.get::<_, String>(3)? == "e2ee" {
                            AccountKind::E2ee
                        } else {
                            AccountKind::Standard
                        },
                        device_id: row.get(4)?,
                        tokens,
                        cursor: row.get(9)?,
                        cursor_id: row.get(12)?,
                        last_sync_at: row.get(10)?,
                        last_error: row.get(11)?,
                    })
                },
            )
            .optional()?)
    }

    /// Links the vault to an account after a sign-in. The same account
    /// again picks up where it left off. Another account (or the first one)
    /// gets everything this vault holds, as one change per document rather
    /// than its whole edit by edit past.
    ///
    /// # Errors
    ///
    /// Database errors, a stored document that does not read.
    pub fn sync_set_link(&mut self, link: &NewLink) -> Result<()> {
        if link.kind == AccountKind::E2ee && link.master_key.is_none() {
            return Err(Error::Invalid("an e2ee account needs its key".into()));
        }
        let now = self.now();
        let key = link_key(&link.server, &link.account_id);
        let tx = self.conn.transaction()?;
        let last: Option<Vec<u8>> = tx
            .query_row(
                "SELECT value FROM vault_meta WHERE key = 'sync_account'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let same = last.as_deref() == Some(key.as_bytes());
        let (cursor, cursor_id): (i64, Option<String>) = if same {
            tx.query_row(
                "SELECT cursor, cursor_id FROM sync_link WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, None))
        } else {
            crate::sync::requeue_all(&tx, &mut self.docs, "", now)?;
            tx.execute("DELETE FROM sync_rejected", [])?;
            tx.execute("DELETE FROM files_on_server", [])?;
            // the shared spaces were the other account's
            let shared = {
                let mut stmt = tx.prepare("SELECT id FROM spaces")?;
                stmt.query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for space in shared {
                super::spaces::drop_space(&tx, &space)?;
            }
            tx.execute(
                "INSERT INTO vault_meta (key, value) VALUES ('sync_account', ?1)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                [key.as_bytes()],
            )?;
            (0, None)
        };
        tx.execute(
            "INSERT OR REPLACE INTO sync_link (id, server, account_id, username, kind, master_key,
                device_id, access_token, access_expires_at, refresh_token, refresh_expires_at,
                cursor, cursor_id, linked_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                link.server,
                link.account_id,
                link.username,
                kind_text(link.kind),
                link.master_key.as_ref().map(|k| k.as_bytes().to_vec()),
                link.device_id,
                link.tokens.access,
                link.tokens.access_expires_at,
                link.tokens.refresh,
                link.tokens.refresh_expires_at,
                cursor,
                cursor_id,
                now,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Sends everything again from the start: the server has less than this
    /// vault took from it (restored from an older backup, or started over).
    /// What it lost comes back from every device that still has it.
    ///
    /// # Errors
    ///
    /// Database errors, a stored document that does not read.
    pub fn sync_resend_all(&mut self, space: &str) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        crate::sync::requeue_all(&tx, &mut self.docs, space, now)?;
        tx.execute(
            if space.is_empty() {
                "UPDATE sync_link SET cursor = 0, cursor_id = NULL WHERE id = 1 AND ?1 = ''"
            } else {
                "UPDATE spaces SET cursor = 0, cursor_id = NULL WHERE id = ?1"
            },
            [space],
        )?;
        // a server that lost changes may have lost files too
        tx.execute("DELETE FROM files_on_server WHERE space = ?1", [space])?;
        tx.commit()?;
        Ok(())
    }

    /// Where a space's sync stands: the number and name of the last change
    /// taken from the server.
    ///
    /// # Errors
    ///
    /// Database errors, a space or link this vault does not have.
    pub fn sync_cursor(&self, space: &str) -> Result<(i64, Option<String>)> {
        let sql = if space.is_empty() {
            "SELECT cursor, cursor_id FROM sync_link WHERE id = 1 AND ?1 = ''"
        } else {
            "SELECT cursor, cursor_id FROM spaces WHERE id = ?1"
        };
        self.conn
            .query_row(sql, [space], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?
            .ok_or_else(|| Error::Invalid("this vault is not linked to an account".into()))
    }

    /// New tokens after a refresh, or `None` once the server signed the
    /// device out. The link stays.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_set_tokens(&mut self, tokens: Option<&Tokens>) -> Result<()> {
        self.conn.execute(
            "UPDATE sync_link SET access_token = ?1, access_expires_at = ?2,
                refresh_token = ?3, refresh_expires_at = ?4 WHERE id = 1",
            params![
                tokens.map(|t| &t.access),
                tokens.map(|t| t.access_expires_at),
                tokens.map(|t| &t.refresh),
                tokens.map(|t| t.refresh_expires_at),
            ],
        )?;
        Ok(())
    }

    /// Stops syncing: the link and its keys are forgotten, the notes stay.
    /// Changes made from now on still queue, a later link to the same
    /// account sends them.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_unlink(&mut self) -> Result<()> {
        self.conn.execute("DELETE FROM sync_link", [])?;
        Ok(())
    }

    /// Notes the end of a round: `None` when it went well.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_finished(&mut self, error: Option<&str>) -> Result<()> {
        let now = self.now();
        match error {
            None => self.conn.execute(
                "UPDATE sync_link SET last_sync_at = ?1, last_error = NULL WHERE id = 1",
                [now],
            )?,
            Some(error) => self
                .conn
                .execute("UPDATE sync_link SET last_error = ?1 WHERE id = 1", [error])?,
        };
        Ok(())
    }

    /// Changes from the server that could not be read.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_rejected(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM sync_rejected", [], |row| row.get(0))?)
    }

    fn cipher(&self, space: &str) -> Result<Cipher> {
        if !space.is_empty() {
            let kind: String = self
                .conn
                .query_row("SELECT kind FROM spaces WHERE id = ?1", [space], |row| {
                    row.get(0)
                })
                .optional()?
                .ok_or(Error::NotFound("space"))?;
            if kind != "e2ee" {
                return Ok(Cipher::Standard);
            }
            let mut stmt = self
                .conn
                .prepare("SELECT epoch, key FROM space_keys WHERE space_id = ?1 ORDER BY epoch")?;
            let keys = stmt
                .query_map([space], |row| {
                    Ok((row.get::<_, u32>(0)?, row.get::<_, Vec<u8>>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .filter_map(|(e, k)| {
                    <[u8; 32]>::try_from(k.as_slice())
                        .ok()
                        .map(|k| (e, Zeroizing::new(k)))
                })
                .collect();
            return Ok(Cipher::Shared(keys));
        }
        let key: Option<Option<Vec<u8>>> = self
            .conn
            .query_row("SELECT master_key FROM sync_link WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        match key {
            None => Err(Error::Invalid(
                "this vault is not linked to an account".into(),
            )),
            Some(None) => Ok(Cipher::Standard),
            Some(Some(bytes)) => MasterKey::from_slice(&bytes)
                .map(Cipher::E2ee)
                .map_err(|_| Error::Corrupt("account key".into())),
        }
    }

    /// The next changes to push, at most `max_changes` and about
    /// `max_bytes` (a single larger change still goes alone), and the outbox
    /// number to acknowledge once the server has them.
    ///
    /// # Errors
    ///
    /// Database errors, a vault that is not linked.
    pub fn sync_push_batch(
        &mut self,
        space: &str,
        max_changes: usize,
        max_bytes: usize,
    ) -> Result<Option<(PushRequest, i64)>> {
        let cipher = self.cipher(space)?;
        let outgoing = crate::sync::outbox(&self.conn, &mut self.docs, Some(space), max_changes)?;
        let mut changes = Vec::new();
        let mut size = 0;
        let mut up_to = None;
        for change in outgoing {
            if !changes.is_empty() && size + change.bytes.len() > max_bytes {
                break;
            }
            size += change.bytes.len();
            let doc = crate::sync::split_key(&change.doc_id).1.to_owned();
            changes.push(OutgoingChange {
                id: change.id,
                data: STANDARD.encode(cipher.seal(&doc, &change.bytes)?),
                doc,
                key: None,
                compacts: change.compacts,
            });
            up_to = Some(change.seq);
        }
        Ok(up_to.map(|seq| (PushRequest { changes }, seq)))
    }

    /// Queues a whole document that replaces its changes on the server up to
    /// this device's cursor. Only when every change of it up to there is
    /// in it: none waiting for an earlier one, none set aside unread, and
    /// not already queued. Returns whether it was queued.
    ///
    /// # Errors
    ///
    /// Database errors, a vault that is not linked.
    pub fn sync_queue_compaction(&mut self, doc_id: &str) -> Result<bool> {
        let now = self.now();
        let cursor: i64 = crate::sync::cursor_of(&self.conn, crate::sync::split_key(doc_id).0)?
            .ok_or_else(|| Error::Invalid("this vault is not linked to an account".into()))?;
        let blocked: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sync_waiting WHERE doc_id = ?1)
                 OR EXISTS (SELECT 1 FROM sync_rejected WHERE doc_id = ?1)
                 OR EXISTS (SELECT 1 FROM sync_outbox WHERE doc_id = ?1 AND compacts IS NOT NULL)",
            [doc_id],
            |row| row.get(0),
        )?;
        if cursor == 0 || blocked || !self.docs.stored(&self.conn, doc_id)? {
            return Ok(false);
        }
        let bytes = crate::sync::snapshot(&self.conn, &mut self.docs, doc_id)?;
        self.conn.execute(
            "INSERT INTO sync_outbox (doc_id, bytes, created_at, compacts) VALUES (?1, ?2, ?3, ?4)",
            params![doc_id, bytes, now, cursor],
        )?;
        Ok(true)
    }

    /// Forgets the documents of notes deleted for good more than
    /// [`crate::sync::FORGET_DELETED_AFTER`] ago, here and on the server.
    /// Returns how many.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn sync_forget_deleted(&mut self) -> Result<u32> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        let forgotten = crate::sync::forget_deleted(&tx, &mut self.docs, now)?;
        tx.commit()?;
        Ok(forgotten)
    }

    /// Takes in a page of changes from the server, in one transaction with
    /// the new cursor. Changes at or before the cursor are skipped, so a
    /// page taken twice changes nothing. A change that cannot be read is set
    /// aside and the rest go on.
    ///
    /// # Errors
    ///
    /// Database errors, a vault that is not linked. Nothing is taken then.
    pub fn sync_take_page(&mut self, space: &str, page: &PullAnswer) -> Result<Taken> {
        let cipher = self.cipher(space)?;
        let now = self.now();
        let device: String =
            self.conn
                .query_row("SELECT device_id FROM sync_link WHERE id = 1", [], |row| {
                    row.get(0)
                })?;
        let (cursor, _) = self.sync_cursor(space)?;
        let rejoin = format!("rejoin:{space}");
        let take_own: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM vault_meta WHERE key = ?1)",
            [&rejoin],
            |row| row.get(0),
        )?;
        let mut taken = Taken::default();
        let mut last = cursor;
        let mut last_id: Option<&str> = None;
        let tx = self.conn.transaction()?;
        let mut touched = crate::sync::Taken::default();
        let done: Result<()> = (|| {
            for change in &page.changes {
                if change.seq <= last {
                    continue;
                }
                last = change.seq;
                last_id = Some(&change.id);
                if !take_own && change.device.as_deref() == Some(device.as_str()) {
                    taken.own += 1;
                    continue;
                }
                let raw = STANDARD.decode(&change.data).unwrap_or_default();
                let opened =
                    if change.doc == SPACE || change.doc.parse::<crate::id::NoteId>().is_ok() {
                        cipher.open(&change.doc, raw.clone())
                    } else {
                        Err("unknown document".to_owned())
                    };
                let refused = match opened {
                    Err(why) => Some(why),
                    Ok(bytes) => {
                        match crate::sync::import_remote(
                            &tx,
                            &mut self.docs,
                            &crate::sync::doc_key(space, &change.doc),
                            &bytes,
                            now,
                            &mut touched,
                        ) {
                            Ok(()) => None,
                            Err(Error::Invalid(why)) => Some(why),
                            // not the change's fault: the page is taken again later
                            Err(other) => return Err(other),
                        }
                    }
                };
                match refused {
                    None => taken.applied += 1,
                    Some(why) => {
                        // an import that fails changes nothing: the
                        // documents taken in before it stay as they are
                        tx.execute(
                            "INSERT OR REPLACE INTO sync_rejected (seq, doc_id, reason, bytes, at)
                             VALUES (?1, ?2, ?3, ?4, ?5)",
                            params![
                                change.seq,
                                crate::sync::doc_key(space, &change.doc),
                                why,
                                raw,
                                now
                            ],
                        )?;
                        taken.rejected += 1;
                    }
                }
            }
            crate::sync::rebuild(&tx, &mut self.docs, std::mem::take(&mut touched), now)?;
            if take_own && !page.more {
                // caught up: the device's own changes are all here again
                tx.execute("DELETE FROM vault_meta WHERE key = ?1", [&rejoin])?;
            }
            if let Some(id) = last_id {
                if space.is_empty() {
                    tx.execute(
                        "UPDATE sync_link SET cursor = ?1, cursor_id = ?2 WHERE id = 1",
                        params![last, id],
                    )?;
                } else {
                    tx.execute(
                        "UPDATE spaces SET cursor = ?1, cursor_id = ?2 WHERE id = ?3",
                        params![last, id, space],
                    )?;
                }
            }
            Ok(())
        })();
        match done.and_then(|()| Ok(tx.commit()?)) {
            Ok(()) => Ok(taken),
            Err(e) => {
                self.docs.forget();
                Err(e)
            }
        }
    }
}
