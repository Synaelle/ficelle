// SPDX-License-Identifier: AGPL-3.0-or-later

//! Storage for what people manage themselves: their confirmed email, a
//! pending deletion, and the export of everything the server keeps for
//! them.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use super::{e2ee, factors};
use crate::db::DbError;

/// The confirmed email of an account.
///
/// # Errors
///
/// Database errors only.
pub fn email(conn: &Connection, user_id: &str) -> Result<Option<String>, DbError> {
    Ok(conn
        .query_row(
            "SELECT email FROM users WHERE id = ?1 AND email_verified_at IS NOT NULL",
            [user_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
}

/// Keeps an address that was just confirmed, `None` clears it.
///
/// # Errors
///
/// Database errors only.
pub fn set_email(
    conn: &Connection,
    user_id: &str,
    email: Option<&str>,
    now: i64,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE users SET email = ?2, email_verified_at = CASE WHEN ?2 IS NULL THEN NULL ELSE ?3 END
         WHERE id = ?1",
        params![user_id, email, now],
    )?;
    super::store::end_resets(conn, user_id)
}

/// When the account goes, if a deletion is pending.
///
/// # Errors
///
/// Database errors only.
pub fn delete_at(conn: &Connection, user_id: &str) -> Result<Option<i64>, DbError> {
    Ok(conn
        .query_row(
            "SELECT delete_at FROM users WHERE id = ?1",
            [user_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
}

/// Sets or clears the pending deletion.
///
/// # Errors
///
/// Database errors only.
pub fn set_delete_at(conn: &Connection, user_id: &str, at: Option<i64>) -> Result<(), DbError> {
    conn.execute(
        "UPDATE users SET delete_at = ?2 WHERE id = ?1",
        params![user_id, at],
    )?;
    Ok(())
}

/// Accounts whose grace period is over: id and username.
///
/// # Errors
///
/// Database errors only.
pub fn due_deletions(conn: &Connection, now: i64) -> Result<Vec<(String, String)>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, username FROM users WHERE delete_at IS NOT NULL AND delete_at <= ?1",
    )?;
    let rows = stmt.query_map([now], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Deletes security log entries older than `before`.
///
/// # Errors
///
/// Database errors only.
pub fn trim_audit(conn: &Connection, before: i64) -> Result<usize, DbError> {
    Ok(conn.execute("DELETE FROM audit_log WHERE at < ?1", [before])?)
}

fn rows(conn: &Connection, sql: &str, user_id: &str, columns: &[&str]) -> Result<Value, DbError> {
    let mut stmt = conn.prepare(sql)?;
    let mut out = Vec::new();
    let mut found = stmt.query([user_id])?;
    while let Some(row) = found.next()? {
        let mut object = serde_json::Map::new();
        for (i, name) in columns.iter().enumerate() {
            let value = match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(n) => json!(n),
                rusqlite::types::ValueRef::Real(f) => json!(f),
                rusqlite::types::ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                rusqlite::types::ValueRef::Blob(_) => continue,
            };
            object.insert((*name).to_owned(), value);
        }
        out.push(Value::Object(object));
    }
    Ok(Value::Array(out))
}

fn sync_usage(conn: &Connection, user_id: &str) -> Result<Value, DbError> {
    let (changes, bytes) = crate::sync::usage(conn, user_id)?;
    Ok(json!({ "changes": changes, "bytes": bytes }))
}

/// Everything the server keeps for an account, as json. Times are unix
/// milliseconds. Secrets stay out (hashes, the OPAQUE record, the TOTP
/// secret), the wrapped e2ee keys are in: only their owner opens them.
///
/// # Errors
///
/// Database errors only.
pub fn export(conn: &Connection, user_id: &str, now: i64) -> Result<Value, DbError> {
    let account = rows(
        conn,
        "SELECT id, username, display_name, role, kind, created_at, email, email_verified_at,
                disabled_at, delete_at
         FROM users WHERE id = ?1",
        user_id,
        &[
            "id",
            "username",
            "displayName",
            "role",
            "kind",
            "createdAt",
            "email",
            "emailConfirmedAt",
            "disabledAt",
            "deleteAt",
        ],
    )?;
    let account = account
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or(Value::Null);
    let devices = rows(
        conn,
        "SELECT name, platform, created_at, last_seen_at, revoked_at FROM devices
         WHERE user_id = ?1 ORDER BY created_at",
        user_id,
        &[
            "name",
            "platform",
            "signedInAt",
            "lastSeenAt",
            "signedOutAt",
        ],
    )?;
    let groups = rows(
        conn,
        "SELECT g.name, m.added_at FROM group_members m JOIN groups g ON g.id = m.group_id
         WHERE m.user_id = ?1 ORDER BY g.name",
        user_id,
        &["name", "addedAt"],
    )?;
    let invites = rows(
        conn,
        "SELECT created_at, expires_at, used_at, revoked_at FROM invites
         WHERE created_by = ?1 ORDER BY created_at",
        user_id,
        &["createdAt", "expiresAt", "usedAt", "revokedAt"],
    )?;
    let passkeys = rows(
        conn,
        "SELECT name, created_at, last_used_at FROM passkeys WHERE user_id = ?1 ORDER BY created_at",
        user_id,
        &["name", "addedAt", "lastUsedAt"],
    )?;
    let sso = rows(
        conn,
        "SELECT issuer, name, created_at FROM sso_identities WHERE user_id = ?1 ORDER BY created_at",
        user_id,
        &["provider", "name", "linkedAt"],
    )?;
    let log = rows(
        conn,
        "SELECT at, event, ip, detail FROM audit_log WHERE user_id = ?1 ORDER BY at",
        user_id,
        &["at", "event", "address", "detail"],
    )?;
    let totp = factors::totp(conn, user_id)?.secret.is_some();
    let codes = factors::recovery_codes_left(conn, user_id)?;
    Ok(json!({
        "format": 1,
        "exportedAt": now,
        "account": account,
        "devices": devices,
        "groups": groups,
        "invitesCreated": invites,
        "signIn": {
            "authenticatorApp": totp,
            "recoveryCodesLeft": codes,
            "passkeys": passkeys,
            "singleSignOn": sso,
        },
        "e2eeKeys": e2ee::keys(conn, user_id)?,
        // the notes themselves come out of the app, as markdown: here only
        // what the server holds of them
        "syncedNotes": sync_usage(conn, user_id)?,
        "securityLog": log,
    }))
}
