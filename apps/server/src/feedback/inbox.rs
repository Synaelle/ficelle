// SPDX-License-Identifier: AGPL-3.0-or-later

//! The moderation inbox over the feedback database: listing with filters,
//! one report whole, its images, marking, keeping, deleting, and the block
//! lists.

use std::fmt::Write;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{BlockKind, block};
use crate::clock::DAY;
use crate::db::DbError;

/// Where a report stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Not looked at.
    New,
    /// Looked at, not done.
    Open,
    /// Fixed, answered or nothing to do.
    Done,
    /// Spam, by the score or the moderator.
    Spam,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Open => "open",
            Self::Done => "done",
            Self::Spam => "spam",
        }
    }
}

/// Which reports to list.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Filter {
    /// One status, every one but spam when absent.
    pub status: Option<Status>,
    /// One category, reports from apps before 0.2.0.
    pub category: Option<String>,
    /// One platform.
    pub platform: Option<String>,
    /// One app version.
    pub version: Option<String>,
    /// Newest first, older than this cursor (`<createdAt>:<id>`).
    pub before: Option<String>,
    /// Oldest first, taken in after this time: for `reports pull`.
    pub after: Option<i64>,
    /// At most this many, 50 by default, 200 at most.
    pub limit: Option<u32>,
}

/// A report in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    id: String,
    created_at: i64,
    status: String,
    /// Empty from apps since 0.2.0.
    category: String,
    /// What it is about, from apps since 0.2.0.
    about: Option<String>,
    /// The first 200 characters.
    excerpt: String,
    version: String,
    platform: String,
    has_contact: bool,
    has_logs: bool,
    images: i64,
    score: i64,
    flags: String,
    kept: bool,
}

/// A page of reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    reports: Vec<Summary>,
    /// The cursor for the next page, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<String>,
    /// Reports by status, whatever the filter.
    counts: Counts,
    /// The versions and platforms seen, for the filters.
    versions: Vec<String>,
    platforms: Vec<String>,
}

/// Reports by status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    new: i64,
    open: i64,
    done: i64,
    spam: i64,
}

fn distinct(conn: &Connection, column: &str) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT DISTINCT {column} FROM reports ORDER BY {column} DESC LIMIT 100"
    ))?;
    Ok(stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

/// Lists reports.
pub fn list(conn: &Connection, filter: &Filter) -> Result<Page, DbError> {
    let limit = i64::from(filter.limit.unwrap_or(50).clamp(1, 200));
    let mut sql = String::from(
        "SELECT id, created_at, status, category, substr(message, 1, 200), version, platform,
             contact IS NOT NULL, logs IS NOT NULL,
             (SELECT count(*) FROM images WHERE report = reports.id), score, flags, kept, about
         FROM reports WHERE 1 = 1",
    );
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    let mut bind = |sql: &mut String, clause: &str, value: rusqlite::types::Value| {
        args.push(value);
        sql.push_str(&clause.replace('?', &format!("?{}", args.len())));
    };
    match filter.status {
        Some(status) => bind(
            &mut sql,
            " AND status = ?",
            status.as_str().to_owned().into(),
        ),
        None => sql.push_str(" AND status != 'spam'"),
    }
    for (column, value) in [
        ("category", &filter.category),
        ("platform", &filter.platform),
        ("version", &filter.version),
    ] {
        if let Some(value) = value {
            bind(
                &mut sql,
                &format!(" AND {column} = ?"),
                value.clone().into(),
            );
        }
    }
    let ascending = filter.after.is_some();
    if let Some(after) = filter.after {
        bind(&mut sql, " AND created_at > ?", after.into());
    } else if let Some((at, id)) = filter.before.as_deref().and_then(|c| c.split_once(':')) {
        let at: i64 = at.parse().unwrap_or(i64::MAX);
        args.push(at.into());
        args.push(id.to_owned().into());
        let (a, b) = (args.len() - 1, args.len());
        let _ = write!(
            sql,
            " AND (created_at < ?{a} OR (created_at = ?{a} AND id < ?{b}))"
        );
    }
    sql.push_str(if ascending {
        " ORDER BY created_at, id"
    } else {
        " ORDER BY created_at DESC, id DESC"
    });
    // one more than asked tells whether there is a next page
    let _ = write!(sql, " LIMIT {}", limit + 1);
    let mut stmt = conn.prepare(&sql)?;
    let mut reports = stmt
        .query_map(rusqlite::params_from_iter(args), |r| {
            Ok(Summary {
                id: r.get(0)?,
                created_at: r.get(1)?,
                status: r.get(2)?,
                category: r.get(3)?,
                about: r.get(13)?,
                excerpt: r.get(4)?,
                version: r.get(5)?,
                platform: r.get(6)?,
                has_contact: r.get(7)?,
                has_logs: r.get(8)?,
                images: r.get(9)?,
                score: r.get(10)?,
                flags: r.get(11)?,
                kept: r.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let more = reports.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    reports.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next = if more && !ascending {
        reports.last().map(|r| format!("{}:{}", r.created_at, r.id))
    } else {
        None
    };
    let mut counts = Counts::default();
    let mut stmt = conn.prepare("SELECT status, count(*) FROM reports GROUP BY status")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (status, n) = row?;
        match status.as_str() {
            "new" => counts.new = n,
            "open" => counts.open = n,
            "done" => counts.done = n,
            _ => counts.spam = n,
        }
    }
    Ok(Page {
        reports,
        next,
        counts,
        versions: distinct(conn, "version")?,
        platforms: distinct(conn, "platform")?,
    })
}

/// An image as listed with its report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageMeta {
    position: i64,
    kind: String,
    width: i64,
    height: i64,
    size: i64,
}

/// One report whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Full {
    id: String,
    created_at: i64,
    status: String,
    /// Empty from apps since 0.2.0.
    category: String,
    /// What it is about, from apps since 0.2.0.
    about: Option<String>,
    message: String,
    /// An email address or a Discord name.
    contact: Option<String>,
    version: String,
    platform: String,
    logs: Option<String>,
    /// The install's id, to block it.
    install: Option<String>,
    score: i64,
    flags: String,
    note: String,
    /// The one answer the sender's app shows, empty until given.
    answer: String,
    /// The sender's app can read the answer: the report came with a receipt.
    answerable: bool,
    kept: bool,
    /// Reports from the same install or address, this one included.
    from_sender: i64,
    images: Vec<ImageMeta>,
}

/// One report, with its images' sizes.
pub fn get(conn: &Connection, id: &str) -> Result<Option<Full>, DbError> {
    let found = conn
        .query_row(
            "SELECT id, created_at, status, category, message, contact, version, platform, logs,
                 install, score, flags, note, kept, about,
                 answer, receipt IS NOT NULL,
                 (SELECT count(*) FROM reports o
                  WHERE o.address = reports.address
                     OR (reports.install IS NOT NULL AND o.install = reports.install))
             FROM reports WHERE id = ?1",
            [id],
            |r| {
                Ok(Full {
                    id: r.get(0)?,
                    created_at: r.get(1)?,
                    status: r.get(2)?,
                    category: r.get(3)?,
                    about: r.get(14)?,
                    message: r.get(4)?,
                    contact: r.get(5)?,
                    version: r.get(6)?,
                    platform: r.get(7)?,
                    logs: r.get(8)?,
                    install: r.get(9)?,
                    score: r.get(10)?,
                    flags: r.get(11)?,
                    note: r.get(12)?,
                    answer: r.get(15)?,
                    answerable: r.get(16)?,
                    kept: r.get(13)?,
                    from_sender: r.get(17)?,
                    images: Vec::new(),
                })
            },
        )
        .optional()?;
    let Some(mut full) = found else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        "SELECT position, kind, width, height, length(bytes) FROM images
         WHERE report = ?1 ORDER BY position",
    )?;
    full.images = stmt
        .query_map([id], |r| {
            Ok(ImageMeta {
                position: r.get(0)?,
                kind: r.get(1)?,
                width: r.get(2)?,
                height: r.get(3)?,
                size: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(full))
}

/// An image's kind and bytes.
pub fn image(
    conn: &Connection,
    id: &str,
    position: i64,
) -> Result<Option<(String, Vec<u8>)>, DbError> {
    Ok(conn
        .query_row(
            "SELECT kind, bytes FROM images WHERE report = ?1 AND position = ?2",
            params![id, position],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// What a moderator changes on a report.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Change {
    /// A new status.
    pub status: Option<Status>,
    /// The moderator's words, 2,000 characters at most.
    pub note: Option<String>,
    /// The one answer the sender's app shows, 2,000 characters at most.
    pub answer: Option<String>,
    /// Kept past the keeping time.
    pub kept: Option<bool>,
}

/// Applies a change. `false` when there is no such report.
pub fn update(conn: &Connection, id: &str, change: &Change) -> Result<bool, DbError> {
    let found = conn.execute(
        "UPDATE reports SET
             status = coalesce(?2, status),
             note = coalesce(?3, note),
             kept = coalesce(?4, kept),
             answer = coalesce(?5, answer)
         WHERE id = ?1",
        params![
            id,
            change.status.map(Status::as_str),
            change.note,
            change.kept,
            change.answer
        ],
    )?;
    Ok(found > 0)
}

/// Deletes a report and its images. `false` when there is no such report.
pub fn delete(conn: &Connection, id: &str) -> Result<bool, DbError> {
    Ok(conn.execute("DELETE FROM reports WHERE id = ?1", [id])? > 0)
}

/// Blocks a report's sender by its install, address or contact, for `days`
/// (never ending without), and files the report as spam. `None` when there
/// is no such report, `Some(false)` when it has no such part.
pub fn block_sender(
    conn: &Connection,
    now: i64,
    id: &str,
    kind: BlockKind,
    days: Option<u32>,
) -> Result<Option<bool>, DbError> {
    let column = match kind {
        BlockKind::Install => "install",
        BlockKind::Address => "address",
        BlockKind::Email => "contact",
        BlockKind::Word => return Ok(Some(false)),
    };
    let value: Option<Option<String>> = conn
        .query_row(
            &format!("SELECT {column} FROM reports WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(value) = value else {
        return Ok(Some(false));
    };
    let expires_at = days.map(|d| now + i64::from(d) * DAY);
    block(
        conn,
        now,
        kind,
        &value,
        &format!("from report {id}"),
        expires_at,
    )?;
    conn.execute("UPDATE reports SET status = 'spam' WHERE id = ?1", [id])?;
    Ok(Some(true))
}

/// A block as the inbox shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    kind: String,
    value: String,
    note: String,
    created_at: i64,
    expires_at: Option<i64>,
}

/// Every block still in force, newest first.
pub fn blocks(conn: &Connection, now: i64) -> Result<Vec<Block>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT kind, value, note, created_at, expires_at FROM blocks
         WHERE expires_at IS NULL OR expires_at > ?1
         ORDER BY created_at DESC, kind, value",
    )?;
    Ok(stmt
        .query_map([now], |r| {
            Ok(Block {
                kind: r.get(0)?,
                value: r.get(1)?,
                note: r.get(2)?,
                created_at: r.get(3)?,
                expires_at: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

/// Lifts a block. `false` when there was none.
pub fn unblock(conn: &Connection, kind: BlockKind, value: &str) -> Result<bool, DbError> {
    Ok(conn.execute(
        "DELETE FROM blocks WHERE kind = ?1 AND value = ?2",
        params![kind.as_str(), value],
    )? > 0)
}
