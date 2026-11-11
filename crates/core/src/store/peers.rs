// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sync with this person's other devices on the same network, the store's
//! side: the devices paired, this device's own key and name, and the three
//! steps of an exchange (versions, the changes the other side lacks, taking
//! changes in). The network is in the `app-peer` crate.
//!
//! A vault syncs either with a server or with its devices, never both:
//! pairing is refused while a server is linked, and linking while a device
//! is paired.

use std::collections::HashMap;

use loro::{ExportMode, VersionVector};
use rusqlite::{OptionalExtension, params};
use zeroize::Zeroizing;

use super::Store;
use crate::error::{Error, Result};
use crate::sync::{SPACE, split_key};

/// Refusal text when pairing meets a server link.
pub const SERVER_LINKED: &str = "this vault syncs with a server";
/// Refusal text when linking meets paired devices.
pub const DEVICES_PAIRED: &str = "this vault syncs with paired devices";

/// A device this vault knows, paired here or learned from one that was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerDevice {
    /// Its long-term public key.
    pub key: Vec<u8>,
    /// What the person calls it.
    pub name: String,
    /// `windows`, `linux`, `macos` or `android`.
    pub platform: String,
    /// The app version it last said, empty when not heard from yet.
    pub version: String,
    /// Where it last answered, `host:port`.
    pub address: Option<String>,
    /// When it was paired, here or on the device this one learned it from.
    pub paired_at: i64,
    /// When a sync with it last ended.
    pub last_sync_at: Option<i64>,
    /// When it was forgotten, a later pairing wins over it.
    pub forgotten_at: Option<i64>,
}

impl PeerDevice {
    /// Still paired: never forgotten, or paired again since.
    pub fn active(&self) -> bool {
        self.forgotten_at.is_none_or(|at| at < self.paired_at)
    }
}

fn meta(conn: &rusqlite::Connection, key: &str) -> Result<Option<Vec<u8>>> {
    Ok(conn
        .query_row(
            "SELECT value FROM vault_meta WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()?)
}

fn set_meta(conn: &rusqlite::Connection, key: &str, value: &[u8]) -> Result<()> {
    conn.execute(
        "INSERT INTO vault_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// A document of the personal space: the space or a note, by its bare id.
fn personal_doc(id: &str) -> bool {
    let (space, bare) = split_key(id);
    space.is_empty() && (bare == SPACE || bare.parse::<crate::id::NoteId>().is_ok())
}

fn vv_error(err: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("not a version of a document: {err}"))
}

impl Store {
    /// This device's long-term secret key, made by the network side at the
    /// first pairing. `None` before.
    ///
    /// # Errors
    ///
    /// Database errors, a stored key of the wrong size.
    pub fn peer_secret(&self) -> Result<Option<Zeroizing<[u8; 32]>>> {
        meta(&self.conn, "peer_secret")?
            .map(|bytes| {
                let key: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| Error::Corrupt("peer key".into()))?;
                Ok(Zeroizing::new(key))
            })
            .transpose()
    }

    /// Keeps this device's long-term secret key.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn set_peer_secret(&mut self, secret: &[u8; 32]) -> Result<()> {
        set_meta(&self.conn, "peer_secret", secret)
    }

    /// The name this device shows other devices, `None` until set.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn peer_name(&self) -> Result<Option<String>> {
        Ok(meta(&self.conn, "peer_name")?.and_then(|b| String::from_utf8(b).ok()))
    }

    /// Renames this device for the others.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for an empty name, database errors.
    pub fn set_peer_name(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("a device needs a name".into()));
        }
        set_meta(&self.conn, "peer_name", name.as_bytes())
    }

    /// Whether a server is linked, signed in or not.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn server_linked(&self) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT EXISTS (SELECT 1 FROM sync_link)", [], |row| {
                row.get(0)
            })?)
    }

    /// Whether any device is paired.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn devices_paired(&self) -> Result<bool> {
        Ok(self.peer_devices()?.iter().any(PeerDevice::active))
    }

    /// Fails when pairing is not allowed here: a server is linked.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] with [`SERVER_LINKED`], database errors.
    pub fn peer_check_pairing(&self) -> Result<()> {
        if self.server_linked()? {
            return Err(Error::Invalid(SERVER_LINKED.into()));
        }
        Ok(())
    }

    /// Every device known, forgotten ones too, by name.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn peer_devices(&self) -> Result<Vec<PeerDevice>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, name, platform, version, address, paired_at, last_sync_at, forgotten_at
             FROM peer_devices ORDER BY name COLLATE NOCASE, key",
        )?;
        let devices = stmt
            .query_map([], |row| {
                Ok(PeerDevice {
                    key: row.get(0)?,
                    name: row.get(1)?,
                    platform: row.get(2)?,
                    version: row.get(3)?,
                    address: row.get(4)?,
                    paired_at: row.get(5)?,
                    last_sync_at: row.get(6)?,
                    forgotten_at: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(devices)
    }

    /// Pins a device just paired, now.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] with [`SERVER_LINKED`] while a server is linked,
    /// database errors.
    pub fn peer_pair(
        &mut self,
        key: &[u8],
        name: &str,
        platform: &str,
        version: &str,
    ) -> Result<()> {
        self.peer_check_pairing()?;
        let now = self.now();
        self.conn.execute(
            "INSERT INTO peer_devices (key, name, platform, version, paired_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (key) DO UPDATE SET name = excluded.name, platform = excluded.platform,
                 version = excluded.version, paired_at = excluded.paired_at",
            params![key, name, platform, version, now],
        )?;
        Ok(())
    }

    /// Takes in another device's list: devices it paired that this one did
    /// not know, and forgettings. The later of pairing and forgetting wins
    /// for each device. `own` is this device's key, never added. Says
    /// whether anything changed.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] with [`SERVER_LINKED`] while a server is linked,
    /// database errors.
    pub fn peer_learn(&mut self, list: &[PeerDevice], own: &[u8]) -> Result<bool> {
        self.peer_check_pairing()?;
        let tx = self.conn.transaction()?;
        let mut changed = 0;
        for device in list.iter().filter(|d| d.key != own && !d.key.is_empty()) {
            changed += tx.execute(
                "INSERT INTO peer_devices (key, name, platform, paired_at, forgotten_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (key) DO UPDATE SET
                     paired_at = max(paired_at, excluded.paired_at),
                     forgotten_at = CASE
                         WHEN forgotten_at IS NULL THEN excluded.forgotten_at
                         WHEN excluded.forgotten_at IS NULL THEN forgotten_at
                         ELSE max(forgotten_at, excluded.forgotten_at) END
                 WHERE excluded.paired_at > paired_at
                     OR coalesce(excluded.forgotten_at, 0) > coalesce(forgotten_at, 0)",
                params![
                    device.key,
                    device.name,
                    device.platform,
                    device.paired_at,
                    device.forgotten_at
                ],
            )?;
            // where the other device reaches it, until it answers here
            changed += tx.execute(
                "UPDATE peer_devices SET address = ?2
                 WHERE key = ?1 AND address IS NULL AND ?2 IS NOT NULL",
                params![device.key, device.address],
            )?;
        }
        tx.commit()?;
        Ok(changed > 0)
    }

    /// Forgets a device: it stops syncing with every device that hears of
    /// it. The notes stay where they are.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] for an unknown key, database errors.
    pub fn peer_forget(&mut self, key: &[u8]) -> Result<()> {
        let now = self.now();
        let done = self.conn.execute(
            "UPDATE peer_devices SET forgotten_at = max(?2, paired_at + 1) WHERE key = ?1",
            params![key, now],
        )?;
        if done == 0 {
            return Err(Error::NotFound("paired device"));
        }
        Ok(())
    }

    /// Notes what a device said when it connected, and when a sync with it
    /// ended.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn peer_seen(
        &mut self,
        key: &[u8],
        name: &str,
        platform: &str,
        version: &str,
        address: Option<&str>,
        synced: bool,
    ) -> Result<()> {
        let now = self.now();
        self.conn.execute(
            "UPDATE peer_devices SET name = ?2, platform = ?3, version = ?4,
                 address = coalesce(?5, address),
                 last_sync_at = CASE WHEN ?6 THEN ?7 ELSE last_sync_at END
             WHERE key = ?1",
            params![key, name, platform, version, address, synced, now],
        )?;
        Ok(())
    }

    /// Notes that a sync with a device just ended.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn peer_synced(&mut self, key: &[u8]) -> Result<()> {
        let now = self.now();
        self.conn.execute(
            "UPDATE peer_devices SET last_sync_at = ?2 WHERE key = ?1",
            params![key, now],
        )?;
        Ok(())
    }

    /// When this device last changed something, for "waiting for" a device
    /// that has not synced since. `None` when nothing changed here yet.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn peer_last_change(&self) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT max(created_at) FROM sync_outbox", [], |row| {
                row.get(0)
            })?)
    }

    /// A mark that moves with every change made here, and only then:
    /// changes taken in from another device leave it alone. `None` when
    /// nothing changed here yet.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn peer_write_mark(&self) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT max(seq) FROM sync_outbox", [], |row| row.get(0))?)
    }

    /// The version of each document of the personal space, Loro's encoding.
    ///
    /// # Errors
    ///
    /// Database errors, a stored document that does not read.
    pub fn peer_versions(&mut self) -> Result<Vec<(String, Vec<u8>)>> {
        let ids: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT id FROM sync_docs UNION SELECT doc_id FROM sync_updates ORDER BY 1",
            )?;
            stmt.query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut versions = Vec::new();
        for id in ids.into_iter().filter(|id| personal_doc(id)) {
            let doc = self.docs.get(&self.conn, &id)?;
            versions.push((id, doc.oplog_vv().encode()));
        }
        Ok(versions)
    }

    /// What the other device lacks, from its versions: changes since its
    /// version of each document, whole documents it does not have. The
    /// space comes first.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for a version that does not read, database errors.
    pub fn peer_changes(&mut self, theirs: &[(String, Vec<u8>)]) -> Result<Vec<(String, Vec<u8>)>> {
        let theirs: HashMap<&str, VersionVector> = theirs
            .iter()
            .map(|(id, bytes)| Ok((id.as_str(), VersionVector::decode(bytes).map_err(vv_error)?)))
            .collect::<Result<_>>()?;
        let mine = self.peer_versions()?;
        let mut changes = Vec::new();
        for (id, _) in mine {
            let doc = self.docs.get(&self.conn, &id)?;
            let bytes = match theirs.get(id.as_str()) {
                None => doc.export(ExportMode::Snapshot),
                Some(vv) if vv.includes_vv(&doc.oplog_vv()) => continue,
                Some(vv) => doc.export(ExportMode::updates(vv)),
            }
            .map_err(|e| Error::Corrupt(format!("sync document: {e}")))?;
            changes.push((id, bytes));
        }
        changes.sort_by_key(|(id, _)| id != SPACE);
        Ok(changes)
    }

    /// Takes in changes from another device in one transaction and rebuilds
    /// what they touch, as a page from a server is. Says how many documents
    /// changed.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for a document outside the personal space or
    /// bytes that are not a change of it (nothing is taken then), database
    /// errors.
    pub fn peer_take(&mut self, changes: &[(String, Vec<u8>)]) -> Result<u32> {
        if let Some((id, _)) = changes.iter().find(|(id, _)| !personal_doc(id)) {
            return Err(Error::Invalid(format!(
                "not a document of this vault: {id}"
            )));
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        let mut taken = crate::sync::Taken::default();
        let done: Result<u32> = (|| {
            let mut grown = 0;
            for (id, bytes) in changes {
                let before = self.docs.get(&tx, id)?.oplog_vv();
                crate::sync::import_remote(&tx, &mut self.docs, id, bytes, &mut taken)?;
                if self.docs.get(&tx, id)?.oplog_vv() != before {
                    grown += 1;
                }
            }
            crate::sync::rebuild(&tx, &mut self.docs, taken, now)?;
            Ok(grown)
        })();
        match done.and_then(|n| Ok(tx.commit().map(|()| n)?)) {
            Ok(n) => Ok(n),
            Err(e) => {
                self.docs.forget();
                Err(e)
            }
        }
    }
}
