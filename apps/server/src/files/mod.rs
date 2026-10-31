// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment files. A space refers to the files its notes use, each stored
//! once as an object in the [`Storage`] backend:
//!
//! - standard spaces: the address is the content's SHA-256, the object is
//!   named by it and shared by every space that uploaded it. It is sealed at
//!   rest with a key derived from the master key and the hash, chunk by
//!   chunk as parts arrive, so the content never sits plain on disk. Each
//!   space uploads the bytes once before it can use them: nobody learns
//!   whether others hold a file by asking for it.
//! - e2ee spaces: the device sealed the file, the address is a keyed hash
//!   only the space's members can make. The object is named by the space
//!   and the address and kept as sent.
//!
//! Released references stay for the owner's retention time (an undo, the
//! trash or a late device can still use the file), then go, then objects no
//! reference uses.

pub mod policy;
pub mod storage;

use std::io::Read;

use app_e2ee::{CHUNK, FILE_HEADER, FileCipher, SEALED_CHUNK};
use app_protocol::e2ee::AccountKind;
use app_protocol::files::{FileState, FileStatus, FilesPolicy, FilesUsage};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::db::DbError;
use crate::keys::MasterKey;
pub use storage::{LocalDisk, Memory, Storage};

/// A file address: 64 lower case hex characters.
pub fn address_ok(address: &str) -> bool {
    address.len() == 64
        && address
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The object a space's file is stored as.
pub fn object_name(kind: AccountKind, space: &str, address: &str) -> String {
    match kind {
        AccountKind::Standard => address.to_owned(),
        AccountKind::E2ee => format!("e-{space}-{address}"),
    }
}

/// The name of an upload under way.
pub fn upload_name(space: &str, address: &str) -> String {
    format!("u-{space}-{address}")
}

/// Bytes a standard file of `plain` bytes takes once sealed at rest.
pub fn stored_size(kind: AccountKind, size: u64) -> u64 {
    match kind {
        AccountKind::Standard => app_e2ee::sealed_size(size),
        AccountKind::E2ee => size,
    }
}

/// Where a file stands for a space.
///
/// # Errors
///
/// Database errors.
pub fn state(conn: &Connection, space: &str, address: &str) -> Result<FileState, DbError> {
    let reference: Option<(i64, Option<i64>)> = conn
        .query_row(
            "SELECT size, released_at FROM file_refs WHERE space_id = ?1 AND address = ?2",
            params![space, address],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((size, released)) = reference {
        return Ok(FileState {
            status: if released.is_some() {
                FileStatus::Kept
            } else {
                FileStatus::Complete
            },
            received: u64::try_from(size).unwrap_or(0),
            size: u64::try_from(size).unwrap_or(0),
        });
    }
    let upload: Option<(i64, i64)> = conn
        .query_row(
            "SELECT size, received FROM file_uploads WHERE space_id = ?1 AND address = ?2",
            params![space, address],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(match upload {
        Some((size, received)) => FileState {
            status: FileStatus::Partial,
            received: u64::try_from(received).unwrap_or(0),
            size: u64::try_from(size).unwrap_or(0),
        },
        None => FileState {
            status: FileStatus::None,
            received: 0,
            size: 0,
        },
    })
}

/// Takes back a file the space released and the server still keeps, when
/// the owner's quota has room for it again.
/// Returns whether there was one, or the refusal.
///
/// # Errors
///
/// Database errors.
pub fn keep(
    conn: &Connection,
    config: &crate::config::FilesConfig,
    space: &str,
    address: &str,
) -> Result<Result<bool, Refusal>, DbError> {
    let size: Option<i64> = conn
        .query_row(
            "SELECT size FROM file_refs
             WHERE space_id = ?1 AND address = ?2 AND released_at IS NOT NULL",
            params![space, address],
            |row| row.get(0),
        )
        .optional()?;
    let Some(size) = size else {
        return Ok(Ok(false));
    };
    // its own bytes count as released while the room is measured
    if room_in(conn, config, space)?
        .is_some_and(|room| u64::try_from(size).unwrap_or(u64::MAX) > room)
    {
        return Ok(Err(Refusal::QuotaFull));
    }
    Ok(Ok(conn.execute(
        "UPDATE file_refs SET released_at = NULL
         WHERE space_id = ?1 AND address = ?2 AND released_at IS NOT NULL",
        params![space, address],
    )? > 0))
}

/// The account that owns a space, whose quota and policy apply.
///
/// # Errors
///
/// Database errors.
pub fn owner_of(conn: &Connection, space: &str) -> Result<Option<String>, DbError> {
    Ok(conn
        .query_row(
            "SELECT owner_id FROM sync_spaces WHERE id = ?1",
            [space],
            |row| row.get(0),
        )
        .optional()?)
}

/// What an account uses: the files of the spaces it owns (a file in two of
/// them once), and the notes of those spaces. Uploads under way are not in
/// it, [`room_for`] counts them.
///
/// # Errors
///
/// Database errors.
pub fn usage(conn: &Connection, owner: &str) -> Result<FilesUsage, DbError> {
    let (file_bytes, files): (i64, i64) = conn.query_row(
        "SELECT coalesce(sum(size), 0), count(*) FROM (
             SELECT DISTINCT r.object, r.size FROM file_refs r
             JOIN sync_spaces s ON s.id = r.space_id
             WHERE s.owner_id = ?1 AND r.released_at IS NULL)",
        [owner],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let notes: i64 = conn.query_row(
        "SELECT coalesce(sum(c.size), 0) FROM sync_changes c
         JOIN sync_spaces s ON s.id = c.space_id WHERE s.owner_id = ?1",
        [owner],
        |row| row.get(0),
    )?;
    let file_bytes = u64::try_from(file_bytes).unwrap_or(0);
    Ok(FilesUsage {
        used_bytes: file_bytes + u64::try_from(notes).unwrap_or(0),
        file_bytes,
        files: u64::try_from(files).unwrap_or(0),
    })
}

/// Why a part is not taken.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The file is larger than the policy allows.
    TooLarge,
    /// The owner's storage would overflow.
    QuotaFull,
    /// Out of order, the wrong size, or past the end: the upload stands at
    /// this many bytes.
    Misplaced(u64),
    /// Not a size a sealed file can have, or parts not cut at chunks.
    Malformed,
}

/// What to do with a part that is taken.
#[derive(Debug, PartialEq, Eq)]
pub enum Part {
    /// The space has the file already, nothing to write.
    Done,
    /// Write it at this offset of the upload.
    Write {
        /// Where the bytes go in the stored upload.
        at: u64,
    },
}

/// Room left in an owner's quota, uploads under way counted. `None` for no
/// limit.
fn room_for(conn: &Connection, owner: &str, quota: u64) -> Result<Option<u64>, DbError> {
    if quota == 0 {
        return Ok(None);
    }
    let pending: i64 = conn.query_row(
        "SELECT coalesce(sum(u.size), 0) FROM file_uploads u
         JOIN sync_spaces s ON s.id = u.space_id WHERE s.owner_id = ?1",
        [owner],
        |row| row.get(0),
    )?;
    // released files stay on disk for the retention time without counting:
    // what goes past one more quota of them counts, so release and upload
    // again fills no disk without end
    let released: i64 = conn.query_row(
        "SELECT coalesce(sum(size), 0) FROM (
             SELECT DISTINCT r.object, r.size FROM file_refs r
             JOIN sync_spaces s ON s.id = r.space_id
             WHERE s.owner_id = ?1 AND r.released_at IS NOT NULL)",
        [owner],
        |row| row.get(0),
    )?;
    let over = u64::try_from(released).unwrap_or(0).saturating_sub(quota);
    let used = usage(conn, owner)?.used_bytes + u64::try_from(pending).unwrap_or(0) + over;
    Ok(Some(quota.saturating_sub(used)))
}

/// Room left for the owner of `space` under their storage quota, notes and
/// files together. `None` for no limit. Note changes check it too, so the
/// quota cannot be gone around by writing notes instead of files.
///
/// # Errors
///
/// Database errors.
pub fn room_in(
    conn: &Connection,
    config: &crate::config::FilesConfig,
    space: &str,
) -> Result<Option<u64>, DbError> {
    let owner = owner_of(conn, space)?.unwrap_or_default();
    let quota = policy::effective(conn, config, &owner)?.quota_bytes;
    room_for(conn, &owner, quota)
}

/// One part of an upload, as the request says.
#[derive(Debug, Clone, Copy)]
pub struct Incoming {
    /// First byte of the part in the file.
    pub start: u64,
    /// Its length.
    pub len: u64,
    /// The whole file's size.
    pub total: u64,
}

/// Checks a part against the upload so far, the policy and the quota, and
/// starts the upload on its first part.
///
/// # Errors
///
/// Database errors, or the [`Refusal`].
pub fn begin_part(
    conn: &Connection,
    kind: AccountKind,
    space: &str,
    address: &str,
    part: Incoming,
    policy: &FilesPolicy,
    now: i64,
) -> Result<Result<Part, Refusal>, DbError> {
    let current = state(conn, space, address)?;
    if current.status == FileStatus::Complete {
        return Ok(Ok(Part::Done));
    }
    let end = part.start.saturating_add(part.len);
    if end > part.total {
        return Ok(Err(Refusal::Misplaced(current.received)));
    }
    let chunked = match kind {
        // standard parts are sealed as they come: whole chunks but the last
        AccountKind::Standard => {
            part.start.is_multiple_of(CHUNK as u64)
                && (end == part.total || part.len.is_multiple_of(CHUNK as u64))
        }
        AccountKind::E2ee => app_e2ee::plain_size(part.total).is_some(),
    };
    if !chunked {
        return Ok(Err(Refusal::Malformed));
    }
    if part.start == 0 {
        let plain = match kind {
            AccountKind::Standard => part.total,
            AccountKind::E2ee => app_e2ee::plain_size(part.total).unwrap_or(part.total),
        };
        if policy.max_file_bytes > 0 && plain > policy.max_file_bytes {
            return Ok(Err(Refusal::TooLarge));
        }
        // a new start drops what came before
        conn.execute(
            "DELETE FROM file_uploads WHERE space_id = ?1 AND address = ?2",
            params![space, address],
        )?;
        let owner = owner_of(conn, space)?.unwrap_or_default();
        if let Some(room) = room_for(conn, &owner, policy.quota_bytes)?
            && part.total > room
        {
            return Ok(Err(Refusal::QuotaFull));
        }
        conn.execute(
            "INSERT INTO file_uploads (space_id, address, size, received, started_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?4)",
            params![
                space,
                address,
                i64::try_from(part.total).unwrap_or(i64::MAX),
                now
            ],
        )?;
    } else if current.status != FileStatus::Partial
        || current.received != part.start
        || current.size != part.total
    {
        return Ok(Err(Refusal::Misplaced(current.received)));
    }
    Ok(Ok(Part::Write {
        at: match kind {
            // the first part carries the header
            AccountKind::Standard if part.start == 0 => 0,
            AccountKind::Standard => {
                FILE_HEADER as u64 + part.start / CHUNK as u64 * SEALED_CHUNK as u64
            }
            AccountKind::E2ee => part.start,
        },
    }))
}

/// The bytes of a part as they are stored: sealed chunk by chunk for a
/// standard file (with the header before the first), as sent for e2ee.
///
/// # Errors
///
/// None in practice: [`begin_part`] checked the sizes.
pub fn stored_part(
    kind: AccountKind,
    master: &MasterKey,
    address: &str,
    part: Incoming,
    bytes: &[u8],
) -> Result<Vec<u8>, app_e2ee::Error> {
    if kind == AccountKind::E2ee {
        return Ok(bytes.to_vec());
    }
    let cipher = FileCipher::new(&master.files_key(), address);
    let chunks = part.total.div_ceil(CHUNK as u64).max(1);
    let first = part.start / CHUNK as u64;
    let mut out = Vec::with_capacity(bytes.len() + FILE_HEADER + bytes.len() / CHUNK * 16 + 16);
    if part.start == 0 {
        out.extend_from_slice(&cipher.header());
    }
    let pieces: Vec<&[u8]> = if bytes.is_empty() {
        vec![&[]]
    } else {
        bytes.chunks(CHUNK).collect()
    };
    for (i, chunk) in pieces.into_iter().enumerate() {
        let index = first + i as u64;
        let position = u32::try_from(index).map_err(|_| app_e2ee::Error::Malformed("file"))?;
        out.extend(cipher.seal_at(position, chunk, index + 1 == chunks)?);
    }
    Ok(out)
}

/// Notes a part as written. Returns whether the upload is complete.
///
/// # Errors
///
/// Database errors.
pub fn part_written(
    conn: &Connection,
    space: &str,
    address: &str,
    part: Incoming,
    now: i64,
) -> Result<bool, DbError> {
    let received = part.start + part.len;
    conn.execute(
        "UPDATE file_uploads SET received = ?3, updated_at = ?4 WHERE space_id = ?1 AND address = ?2",
        params![
            space,
            address,
            i64::try_from(received).unwrap_or(i64::MAX),
            now
        ],
    )?;
    Ok(received == part.total)
}

/// Whether a complete standard upload holds the content its address names:
/// every chunk opens and the content hashes to the address.
///
/// # Errors
///
/// Read errors from the backend.
pub fn check_standard(
    source: &mut dyn Read,
    master: &MasterKey,
    address: &str,
    total: u64,
) -> std::io::Result<bool> {
    let cipher = FileCipher::new(&master.files_key(), address);
    let mut header = [0u8; FILE_HEADER];
    source.read_exact(&mut header)?;
    if header != cipher.header() {
        return Ok(false);
    }
    let chunks = total.div_ceil(CHUNK as u64).max(1);
    let mut hasher = Sha256::new();
    let mut sealed = vec![0u8; SEALED_CHUNK];
    for index in 0..chunks {
        let last = index + 1 == chunks;
        let len = if last {
            usize::try_from(total - index * CHUNK as u64).unwrap_or(0) + 16
        } else {
            SEALED_CHUNK
        };
        source.read_exact(&mut sealed[..len])?;
        let Ok(position) = u32::try_from(index) else {
            return Ok(false);
        };
        match cipher.open_at(position, &sealed[..len], last) {
            Ok(plain) => hasher.update(&plain),
            Err(_) => return Ok(false),
        }
    }
    // nothing after the last chunk
    let mut extra = [0u8; 1];
    if source.read(&mut extra)? != 0 {
        return Ok(false);
    }
    Ok(hex(&hasher.finalize()) == address)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            // writing to a String cannot fail
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Records a complete upload: the object (once), the space's reference,
/// and the upload gone. Run before the bytes are moved into place, so the
/// sweep never takes an object a reference is about to use.
///
/// # Errors
///
/// Database errors.
pub fn completed(
    conn: &Connection,
    kind: AccountKind,
    space: &str,
    address: &str,
    total: u64,
    now: i64,
) -> Result<String, DbError> {
    let object = object_name(kind, space, address);
    let size = i64::try_from(total).unwrap_or(i64::MAX);
    let stored = i64::try_from(stored_size(kind, total)).unwrap_or(i64::MAX);
    conn.execute(
        "INSERT OR IGNORE INTO files (object, size, stored, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![object, size, stored, now],
    )?;
    conn.execute(
        "INSERT INTO file_refs (space_id, address, object, size, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (space_id, address) DO UPDATE SET released_at = NULL",
        params![space, address, object, size, now],
    )?;
    conn.execute(
        "DELETE FROM file_uploads WHERE space_id = ?1 AND address = ?2",
        params![space, address],
    )?;
    Ok(object)
}

/// Drops an upload whose content did not check out.
///
/// # Errors
///
/// Database errors.
pub fn abandon(conn: &Connection, space: &str, address: &str) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM file_uploads WHERE space_id = ?1 AND address = ?2",
        params![space, address],
    )?;
    Ok(())
}

/// A file a space can read: its object and size, released ones included
/// while they are kept.
///
/// # Errors
///
/// Database errors.
pub fn readable(
    conn: &Connection,
    space: &str,
    address: &str,
) -> Result<Option<(String, u64)>, DbError> {
    Ok(conn
        .query_row(
            "SELECT object, size FROM file_refs WHERE space_id = ?1 AND address = ?2",
            params![space, address],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .map(|(object, size)| (object, u64::try_from(size).unwrap_or(0))))
}

/// Lets a space's file go, if the space has not moved on since the device
/// looked (`head`): a change pushed since may use the file. `Ok(false)`
/// when it moved on.
///
/// # Errors
///
/// Database errors.
pub fn release(
    conn: &Connection,
    space: &str,
    address: &str,
    head: i64,
    now: i64,
) -> Result<bool, DbError> {
    if crate::sync::head(conn, space)? != head {
        return Ok(false);
    }
    conn.execute(
        "UPDATE file_refs SET released_at = ?3
         WHERE space_id = ?1 AND address = ?2 AND released_at IS NULL",
        params![space, address, now],
    )?;
    Ok(true)
}

/// Forgets references released longer ago than their owner keeps them
/// (`retention` in days for an owner id), then objects no reference uses,
/// and uploads left untouched for a day. Returns the objects and uploads
/// for the backend to delete.
///
/// # Errors
///
/// Database errors.
pub fn sweep(
    conn: &Connection,
    now: i64,
    retention: &dyn Fn(&Connection, &str) -> Result<u32, DbError>,
) -> Result<(Vec<String>, Vec<String>), DbError> {
    let released: Vec<(String, String, i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT r.space_id, r.address, r.released_at, s.owner_id FROM file_refs r
             JOIN sync_spaces s ON s.id = r.space_id WHERE r.released_at IS NOT NULL",
        )?;
        stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    for (space, address, at, owner) in released {
        let keep = i64::from(retention(conn, &owner)?) * crate::clock::DAY;
        if at + keep <= now {
            conn.execute(
                "DELETE FROM file_refs WHERE space_id = ?1 AND address = ?2",
                params![space, address],
            )?;
        }
    }
    let objects: Vec<String> = {
        let mut stmt = conn.prepare(
            "DELETE FROM files WHERE object NOT IN (SELECT object FROM file_refs) RETURNING object",
        )?;
        stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let uploads: Vec<String> = {
        let mut stmt = conn.prepare(
            "DELETE FROM file_uploads WHERE updated_at < ?1 RETURNING space_id, address",
        )?;
        stmt.query_map([now - crate::clock::DAY], |row| {
            Ok(upload_name(
                &row.get::<_, String>(0)?,
                &row.get::<_, String>(1)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    Ok((objects, uploads))
}

/// Uploads the database still knows, so the backend's leftovers can go.
///
/// # Errors
///
/// Database errors.
pub fn live_uploads(conn: &Connection) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare("SELECT space_id, address FROM file_uploads")?;
    let names = stmt
        .query_map([], |row| {
            Ok(upload_name(
                &row.get::<_, String>(0)?,
                &row.get::<_, String>(1)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

/// The instance's storage, and every person's use, most first.
///
/// # Errors
///
/// Database errors.
pub fn storage(
    conn: &Connection,
    config: &crate::config::FilesConfig,
    offset: u32,
    limit: u32,
) -> Result<app_protocol::admin::StorageOverview, DbError> {
    use app_protocol::admin::{Page, PersonStorage, StorageOverview};
    let size = |n: i64| u64::try_from(n).unwrap_or(0);
    let (stored, files, unique): (i64, i64, i64) = conn.query_row(
        "SELECT coalesce(sum(stored), 0), count(*), coalesce(sum(size), 0) FROM files",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let referenced: i64 =
        conn.query_row("SELECT coalesce(sum(size), 0) FROM file_refs", [], |row| {
            row.get(0)
        })?;
    let notes: i64 = conn.query_row(
        "SELECT coalesce(sum(size), 0) FROM sync_changes",
        [],
        |row| row.get(0),
    )?;
    let mut people: Vec<PersonStorage> = {
        let mut stmt = conn.prepare(
            "WITH used AS (
                 SELECT DISTINCT s.owner_id, r.object, r.size FROM file_refs r
                 JOIN sync_spaces s ON s.id = r.space_id WHERE r.released_at IS NULL),
             files AS (
                 SELECT owner_id, sum(size) AS bytes, count(*) AS n, max(size) AS largest
                 FROM used GROUP BY owner_id),
             notes AS (
                 SELECT s.owner_id, sum(c.size) AS bytes FROM sync_changes c
                 JOIN sync_spaces s ON s.id = c.space_id GROUP BY s.owner_id)
             SELECT u.id, u.username, u.display_name, coalesce(f.bytes, 0), coalesce(f.n, 0),
                 coalesce(f.largest, 0), coalesce(n.bytes, 0)
             FROM users u LEFT JOIN files f ON f.owner_id = u.id LEFT JOIN notes n ON n.owner_id = u.id",
        )?;
        stmt.query_map([], |row| {
            let file_bytes = size(row.get(3)?);
            Ok(PersonStorage {
                user_id: row.get(0)?,
                username: row.get(1)?,
                display_name: row.get(2)?,
                used_bytes: file_bytes + size(row.get(6)?),
                file_bytes,
                files: size(row.get(4)?),
                largest_bytes: size(row.get(5)?),
                quota_bytes: 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    people.sort_by(|a, b| {
        b.used_bytes
            .cmp(&a.used_bytes)
            .then_with(|| a.username.cmp(&b.username))
    });
    let total = people.len() as u64;
    let mut items: Vec<PersonStorage> = people
        .into_iter()
        .skip(usize::try_from(offset).unwrap_or(usize::MAX))
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .collect();
    for person in &mut items {
        person.quota_bytes = policy::effective(conn, config, &person.user_id)?.quota_bytes;
    }
    Ok(StorageOverview {
        stored_bytes: size(stored),
        files: size(files),
        referenced_bytes: size(referenced),
        unique_bytes: size(unique),
        note_bytes: size(notes),
        people: Page { total, items },
    })
}

/// Reads a standard file's content from `offset`, opening the sealed object
/// chunk by chunk.
pub struct PlainReader {
    source: Box<dyn Read + Send>,
    cipher: FileCipher,
    /// Next chunk to open.
    index: u64,
    chunks: u64,
    total: u64,
    /// Bytes to drop from the first chunk.
    skip: usize,
    buf: Vec<u8>,
    at: usize,
}

impl PlainReader {
    /// `source` reads the object from the start of chunk `offset / CHUNK`.
    pub fn new(
        source: Box<dyn Read + Send>,
        master: &MasterKey,
        address: &str,
        total: u64,
        offset: u64,
    ) -> Self {
        Self {
            source,
            cipher: FileCipher::new(&master.files_key(), address),
            index: offset / CHUNK as u64,
            chunks: total.div_ceil(CHUNK as u64).max(1),
            total,
            skip: usize::try_from(offset % CHUNK as u64).unwrap_or(0),
            buf: Vec::new(),
            at: 0,
        }
    }

    /// Where chunk `offset / CHUNK` starts in the stored object.
    pub fn stored_offset(offset: u64) -> u64 {
        FILE_HEADER as u64 + offset / CHUNK as u64 * SEALED_CHUNK as u64
    }
}

impl Read for PlainReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.at == self.buf.len() {
            if self.index >= self.chunks {
                return Ok(0);
            }
            let last = self.index + 1 == self.chunks;
            let len = if last {
                usize::try_from(self.total - self.index * CHUNK as u64).unwrap_or(0) + 16
            } else {
                SEALED_CHUNK
            };
            let mut sealed = vec![0u8; len];
            self.source.read_exact(&mut sealed)?;
            let position = u32::try_from(self.index).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "file too long")
            })?;
            let plain = self.cipher.open_at(position, &sealed, last).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "damaged file")
            })?;
            self.buf.clear();
            self.buf
                .extend_from_slice(&plain[self.skip.min(plain.len())..]);
            self.skip = 0;
            self.at = 0;
            self.index += 1;
            if self.buf.is_empty() {
                return Ok(0);
            }
        }
        let n = out.len().min(self.buf.len() - self.at);
        out[..n].copy_from_slice(&self.buf[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_parts_seal_to_the_whole_file() {
        let master = MasterKey::generate();
        let content: Vec<u8> = (0..CHUNK * 3 + 100)
            .map(|i| u8::try_from(i % 241).unwrap())
            .collect();
        let address = hex(&Sha256::digest(&content));
        let total = content.len() as u64;
        // parts of two chunks, then the rest
        let mut stored = Vec::new();
        for start in (0..content.len()).step_by(2 * CHUNK) {
            let end = (start + 2 * CHUNK).min(content.len());
            let part = Incoming {
                start: start as u64,
                len: (end - start) as u64,
                total,
            };
            stored.extend(
                stored_part(
                    AccountKind::Standard,
                    &master,
                    &address,
                    part,
                    &content[start..end],
                )
                .unwrap(),
            );
        }
        assert_eq!(
            stored.len() as u64,
            stored_size(AccountKind::Standard, total)
        );
        assert!(check_standard(&mut &stored[..], &master, &address, total).unwrap());
        // read back from the middle
        let offset = CHUNK as u64 + 10;
        let from = usize::try_from(PlainReader::stored_offset(offset)).unwrap();
        let mut reader = PlainReader::new(
            Box::new(std::io::Cursor::new(stored[from..].to_vec())),
            &master,
            &address,
            total,
            offset,
        );
        let mut back = Vec::new();
        reader.read_to_end(&mut back).unwrap();
        assert_eq!(back, content[usize::try_from(offset).unwrap()..]);
        // another content under that address does not check out
        let mut wrong = stored.clone();
        wrong[FILE_HEADER + 3] ^= 1;
        assert!(!check_standard(&mut &wrong[..], &master, &address, total).unwrap());
    }

    #[test]
    fn an_empty_file_is_one_empty_chunk() {
        let master = MasterKey::generate();
        let address = hex(&Sha256::digest(b""));
        let part = Incoming {
            start: 0,
            len: 0,
            total: 0,
        };
        let stored = stored_part(AccountKind::Standard, &master, &address, part, &[]).unwrap();
        assert!(check_standard(&mut &stored[..], &master, &address, 0).unwrap());
    }
}
