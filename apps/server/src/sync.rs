// SPDX-License-Identifier: AGPL-3.0-or-later

//! Storage for sync: each account's space and its log of changes. The
//! server never merges or reads documents, it keeps changes in the order it
//! took them and hands them back. For a standard space it encrypts each
//! change at rest with the space key, for an e2ee space the change arrives
//! encrypted and is kept as sent.

use app_protocol::e2ee::AccountKind;
use rusqlite::{Connection, OptionalExtension, params};
use zeroize::Zeroizing;

use crate::db::DbError;
use crate::keys::{MasterKey, open, seal};

/// A space as stored.
#[derive(Debug, Clone)]
pub struct Space {
    /// Id, the owner's account id for a personal space.
    pub id: String,
    /// Standard or e2ee.
    pub kind: AccountKind,
    /// Standard: the space key wrapped by the master key.
    pub data_key: Option<Vec<u8>>,
}

/// One change to keep.
pub struct NewChange<'a> {
    /// The device's id for it.
    pub change_id: &'a str,
    /// The document.
    pub doc_id: &'a str,
    /// The bytes as sent (a Loro update, or e2ee ciphertext).
    pub bytes: &'a [u8],
    /// E2ee: the document key, wrapped.
    pub key: Option<&'a str>,
    /// Holds all of its document up to this number, see [`push`].
    pub compacts: Option<i64>,
}

/// One change read back, decrypted for a standard space.
pub struct StoredChange {
    /// Number in the log.
    pub seq: i64,
    /// The device's name for it.
    pub change_id: String,
    /// The document.
    pub doc_id: String,
    /// The bytes as the device sent them.
    pub bytes: Zeroizing<Vec<u8>>,
    /// The device that sent it.
    pub device_id: Option<String>,
}

fn change_label(space: &str, doc: &str, change: &str) -> Vec<u8> {
    [
        space.as_bytes(),
        b"\0",
        doc.as_bytes(),
        b"\0",
        change.as_bytes(),
    ]
    .concat()
}

/// The account's space, made on first use. A standard space gets its key.
///
/// # Errors
///
/// Database errors only.
pub fn space_of(
    conn: &Connection,
    master: &MasterKey,
    user_id: &str,
    kind: AccountKind,
    now: i64,
) -> Result<Space, DbError> {
    let found: Option<Option<Vec<u8>>> = conn
        .query_row(
            "SELECT data_key FROM sync_spaces WHERE id = ?1",
            [user_id],
            |row| row.get(0),
        )
        .optional()?;
    let data_key = if let Some(key) = found {
        key
    } else {
        let wrapped = match kind {
            AccountKind::Standard => Some(master.new_space_key(user_id).1),
            AccountKind::E2ee => None,
        };
        conn.execute(
            "INSERT INTO sync_spaces (id, owner_id, data_key, created_at) VALUES (?1, ?1, ?2, ?3)",
            params![user_id, wrapped, now],
        )?;
        wrapped
    };
    Ok(Space {
        id: user_id.to_owned(),
        kind,
        data_key,
    })
}

/// A space's key wrapped by the master key, none for e2ee.
///
/// # Errors
///
/// Database errors only.
pub fn data_key(conn: &Connection, space: &str) -> Result<Option<Vec<u8>>, DbError> {
    Ok(conn
        .query_row(
            "SELECT data_key FROM sync_spaces WHERE id = ?1",
            [space],
            |row| row.get(0),
        )
        .optional()?
        .flatten())
}

/// Whether open notes may follow other devices' edits live. On unless an
/// admin turned it off.
///
/// # Errors
///
/// Database errors only.
pub fn coediting(conn: &Connection) -> Result<bool, DbError> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'coediting'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.as_deref() != Some("off"))
}

/// Turns co-editing on or off for the whole server.
///
/// # Errors
///
/// Database errors only.
pub fn set_coediting(conn: &Connection, on: bool) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('coediting', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [if on { "on" } else { "off" }],
    )?;
    Ok(())
}

/// Number of the latest change of a space, 0 when empty.
///
/// # Errors
///
/// Database errors only.
pub fn head(conn: &Connection, space: &str) -> Result<i64, DbError> {
    Ok(conn.query_row(
        "SELECT coalesce(max(seq), 0) FROM sync_changes WHERE space_id = ?1",
        [space],
        |row| row.get(0),
    )?)
}

fn space_key(master: &MasterKey, space: &Space) -> Result<Option<Zeroizing<[u8; 32]>>, DbError> {
    match &space.data_key {
        Some(wrapped) => master
            .space_key(&space.id, wrapped)
            .map(Some)
            .ok_or_else(|| {
                DbError::Data("the space key does not open with this master key".into())
            }),
        None => Ok(None),
    }
}

/// Keeps changes in order. A change whose id the space already has is
/// skipped: a device resending after a lost answer stores nothing twice.
/// Returns how many were new.
///
/// # Errors
///
/// Database errors, a space key that does not open.
pub fn push(
    conn: &Connection,
    master: &MasterKey,
    space: &Space,
    device_id: &str,
    changes: &[NewChange<'_>],
    now: i64,
) -> Result<u32, DbError> {
    let key = space_key(master, space)?;
    let mut accepted = 0;
    for change in changes {
        let stored = match &key {
            Some(key) => seal(
                key,
                change.bytes,
                &change_label(&space.id, change.doc_id, change.change_id),
            ),
            None => change.bytes.to_vec(),
        };
        let size = i64::try_from(change.bytes.len()).unwrap_or(i64::MAX);
        let new = conn.execute(
            "INSERT OR IGNORE INTO sync_changes (space_id, doc_id, device_id, change_id, bytes, size, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![space.id, change.doc_id, device_id, change.change_id, stored, size, now],
        )?;
        accepted += new;
        if let (1, Some(up_to)) = (new, change.compacts) {
            // the rows stay, empty: a resend of one is still known, and a
            // device's cursor still names a change
            conn.execute(
                "UPDATE sync_changes SET bytes = x'', size = 0, compacted = 1
                 WHERE space_id = ?1 AND doc_id = ?2 AND seq <= ?3 AND compacted = 0
                   AND change_id != ?4",
                params![space.id, change.doc_id, up_to, change.change_id],
            )?;
        }
        if let Some(wrapped) = change.key {
            conn.execute(
                "INSERT OR IGNORE INTO sync_doc_keys (space_id, doc_id, wrapped) VALUES (?1, ?2, ?3)",
                params![space.id, change.doc_id, wrapped],
            )?;
        }
    }
    Ok(u32::try_from(accepted).unwrap_or(u32::MAX))
}

/// The bytes a compaction of `doc` up to `up_to` would empty: what it may
/// add without counting against the quota.
///
/// # Errors
///
/// Database errors only.
pub fn compaction_frees(
    conn: &Connection,
    space_id: &str,
    doc_id: &str,
    up_to: i64,
) -> Result<u64, DbError> {
    let freed: i64 = conn.query_row(
        "SELECT COALESCE(SUM(size), 0) FROM sync_changes
         WHERE space_id = ?1 AND doc_id = ?2 AND seq <= ?3 AND compacted = 0",
        params![space_id, doc_id, up_to],
        |row| row.get(0),
    )?;
    Ok(u64::try_from(freed).unwrap_or(0))
}

/// About the most change bytes in one page.
pub const PAGE_BYTES: usize = 8 * 1024 * 1024;

/// Changes after `after`, oldest first, at most `limit` and about
/// [`PAGE_BYTES`] (a single larger change still comes alone), and whether
/// more wait.
///
/// # Errors
///
/// Database errors, stored bytes that do not open with the space key.
pub fn pull(
    conn: &Connection,
    master: &MasterKey,
    space: &Space,
    after: i64,
    limit: usize,
) -> Result<(Vec<StoredChange>, bool), DbError> {
    let key = space_key(master, space)?;
    let mut stmt = conn.prepare(
        "SELECT seq, doc_id, change_id, bytes, device_id FROM sync_changes
         WHERE space_id = ?1 AND seq > ?2 AND compacted = 0 ORDER BY seq LIMIT ?3",
    )?;
    let fetch = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
    let rows = stmt
        .query_map(params![space.id, after, fetch], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut more = rows.len() > limit;
    let mut out = Vec::with_capacity(rows.len().min(limit));
    let mut size = 0;
    for (seq, doc_id, change_id, bytes, device_id) in rows.into_iter().take(limit) {
        // big changes (whole notes sent by a newly linked device) end a page
        // early, an answer stays a few megabytes whatever the count
        if !out.is_empty() && size + bytes.len() > PAGE_BYTES {
            more = true;
            break;
        }
        size += bytes.len();
        let bytes = match &key {
            Some(key) => open(key, &bytes, &change_label(&space.id, &doc_id, &change_id))
                .ok_or_else(|| DbError::Data(format!("change {seq} does not open")))?,
            None => Zeroizing::new(bytes),
        };
        out.push(StoredChange {
            seq,
            change_id,
            doc_id,
            bytes,
            device_id,
        });
    }
    Ok((out, more))
}

/// Name of the change numbered `seq` in a space, if there is one.
///
/// # Errors
///
/// Database errors only.
pub fn change_at(conn: &Connection, space: &str, seq: i64) -> Result<Option<String>, DbError> {
    Ok(conn
        .query_row(
            "SELECT change_id FROM sync_changes WHERE space_id = ?1 AND seq = ?2",
            params![space, seq],
            |row| row.get(0),
        )
        .optional()?)
}

/// Documents of a space with at least `min` changes not compacted, most
/// first.
///
/// # Errors
///
/// Database errors only.
pub fn heavy(conn: &Connection, space: &str, min: i64) -> Result<Vec<(String, i64)>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT doc_id, count(*) FROM sync_changes WHERE space_id = ?1 AND compacted = 0
         GROUP BY doc_id HAVING count(*) >= ?2 ORDER BY 2 DESC LIMIT 100",
    )?;
    let rows = stmt
        .query_map(params![space, min], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The wrapped keys of these documents, e2ee only.
///
/// # Errors
///
/// Database errors only.
pub fn doc_keys(
    conn: &Connection,
    space: &str,
    docs: &[&str],
) -> Result<Vec<(String, String)>, DbError> {
    let mut stmt =
        conn.prepare("SELECT wrapped FROM sync_doc_keys WHERE space_id = ?1 AND doc_id = ?2")?;
    let mut out = Vec::new();
    for doc in docs {
        if let Some(wrapped) = stmt
            .query_row(params![space, doc], |row| row.get::<_, String>(0))
            .optional()?
        {
            out.push(((*doc).to_owned(), wrapped));
        }
    }
    Ok(out)
}

/// Space use of an account: changes and their size, for the admin page and
/// the export.
///
/// # Errors
///
/// Database errors only.
pub fn usage(conn: &Connection, space: &str) -> Result<(i64, i64), DbError> {
    Ok(conn.query_row(
        "SELECT count(*), coalesce(sum(size), 0) FROM sync_changes WHERE space_id = ?1",
        [space],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}
