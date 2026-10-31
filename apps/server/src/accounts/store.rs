// SPDX-License-Identifier: AGPL-3.0-or-later

//! Account storage. Plain functions over a connection, each given `now` so
//! tests control time. Callers run them in [`crate::db::Db::call`], several
//! in one transaction where they must succeed together.

use app_protocol::account::{DeviceInfo, Registration, Role};
use app_protocol::e2ee::AccountKind;
use rusqlite::{Connection, OptionalExtension, params};

use super::token::{self, Kind, TokenHash};
use crate::clock::{DAY, MINUTE};
use crate::config::AuthConfig;
use crate::db::DbError;

/// A stored account.
#[derive(Debug, Clone)]
pub struct UserRow {
    /// Id.
    pub id: String,
    /// Lower case username.
    pub username: String,
    /// Shown name.
    pub display_name: String,
    /// PHC string.
    pub password_hash: String,
    /// Role.
    pub role: Role,
    /// Creation time.
    pub created_at: i64,
    /// Set when an admin turned the account off.
    pub disabled_at: Option<i64>,
    /// Set while sign-ins are refused.
    pub locked_until: Option<i64>,
    /// Standard or end-to-end encrypted.
    pub kind: AccountKind,
    /// Must add a second factor before acting as an admin.
    pub factor_required: bool,
}

impl UserRow {
    /// An admin who may act as one: not waiting for a second factor.
    pub fn acts_as_admin(&self) -> bool {
        self.role == Role::Admin && !self.factor_required
    }
}

/// A new pair of tokens for a device.
#[derive(Debug, Clone)]
pub struct Tokens {
    /// Device id.
    pub device_id: String,
    /// Access token, plain, to hand out once.
    pub access: String,
    /// When it expires.
    pub access_expires_at: i64,
    /// Refresh token, plain, to hand out once.
    pub refresh: String,
    /// When it expires.
    pub refresh_expires_at: i64,
}

/// Why a refresh token was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum RefreshRefused {
    /// Unknown, expired, or its device was signed out.
    Invalid,
    /// Already used once: someone else has a copy. The device is now signed
    /// out everywhere.
    Reused {
        /// Account of the device.
        user_id: String,
        /// Device signed out.
        device_id: String,
    },
}

/// A signed in request.
#[derive(Debug, Clone)]
pub struct Authed {
    /// The account.
    pub user: UserRow,
    /// The device.
    pub device_id: String,
}

/// A signed in device.
#[derive(Debug, Clone)]
pub struct DeviceRow {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Platform.
    pub platform: String,
    /// First sign-in.
    pub created_at: i64,
    /// Last request.
    pub last_seen_at: i64,
}

/// A stored invite.
#[derive(Debug, Clone)]
pub struct InviteRow {
    /// Id.
    pub id: String,
    /// Username of the creator, empty if the account is gone.
    pub created_by: String,
    /// Creation time.
    pub created_at: i64,
    /// Expiry.
    pub expires_at: i64,
    /// `pending`, `used`, `revoked` or `expired`.
    pub state: &'static str,
}

/// A refresh token with its device: device id, user id, expiry, use time
/// and the device's sign-out time.
type RefreshRow = (String, String, i64, Option<i64>, Option<i64>);

/// Device `last_seen_at` is written at most this often, not on every request.
const SEEN_EVERY: i64 = 5 * MINUTE;

fn role_of(text: &str) -> Role {
    if text == "admin" {
        Role::Admin
    } else {
        Role::User
    }
}

fn role_text(role: Role) -> &'static str {
    match role {
        Role::Admin => "admin",
        Role::User => "user",
    }
}

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

const USER_COLUMNS: &str = "id, username, display_name, password_hash, role, created_at, disabled_at, locked_until, kind, \
     factor_required";

fn user_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserRow> {
    Ok(UserRow {
        id: row.get(0)?,
        username: row.get(1)?,
        display_name: row.get(2)?,
        password_hash: row.get(3)?,
        role: role_of(&row.get::<_, String>(4)?),
        created_at: row.get(5)?,
        disabled_at: row.get(6)?,
        locked_until: row.get(7)?,
        kind: super::e2ee::kind_of(&row.get::<_, String>(8)?),
        factor_required: row.get(9)?,
    })
}

/// The account with this username, any letter case.
///
/// # Errors
///
/// Database errors only.
pub fn user_by_name(conn: &Connection, username: &str) -> Result<Option<UserRow>, DbError> {
    let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE username = ?1");
    Ok(conn.query_row(&sql, [username], user_from).optional()?)
}

/// The account with this id.
///
/// # Errors
///
/// Database errors only.
pub fn user_by_id(conn: &Connection, id: &str) -> Result<Option<UserRow>, DbError> {
    let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1");
    Ok(conn.query_row(&sql, [id], user_from).optional()?)
}

/// Creates an account. `None` when the username is taken.
///
/// # Errors
///
/// Database errors only.
pub fn insert_user(
    conn: &Connection,
    username: &str,
    display_name: &str,
    password_hash: &str,
    role: Role,
    now: i64,
) -> Result<Option<UserRow>, DbError> {
    insert_user_with_id(
        conn,
        &new_id(),
        username,
        display_name,
        password_hash,
        role,
        now,
    )
}

/// [`insert_user`] with an id chosen beforehand: setup registers the
/// admin's passkey before the account exists, under that id.
///
/// # Errors
///
/// Database errors only.
pub fn insert_user_with_id(
    conn: &Connection,
    id: &str,
    username: &str,
    display_name: &str,
    password_hash: &str,
    role: Role,
    now: i64,
) -> Result<Option<UserRow>, DbError> {
    if user_by_name(conn, username)?.is_some() {
        return Ok(None);
    }
    conn.execute(
        "INSERT INTO users (id, username, display_name, password_hash, role, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            username,
            display_name,
            password_hash,
            role_text(role),
            now
        ],
    )?;
    user_by_id(conn, id)
}

/// Sets or clears the need for a second factor.
///
/// # Errors
///
/// Database errors only.
pub fn set_factor_required(
    conn: &Connection,
    user_id: &str,
    required: bool,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE users SET factor_required = ?2 WHERE id = ?1",
        params![user_id, required],
    )?;
    Ok(())
}

/// Stores a new password hash.
///
/// # Errors
///
/// Database errors only.
pub fn set_password(conn: &Connection, user_id: &str, hash: &str) -> Result<(), DbError> {
    conn.execute(
        "UPDATE users SET password_hash = ?2 WHERE id = ?1",
        params![user_id, hash],
    )?;
    end_resets(conn, user_id)
}

/// Reset links still waiting go: a new password or email makes them
/// answer to something past.
///
/// # Errors
///
/// Database errors only.
pub fn end_resets(conn: &Connection, user_id: &str) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM challenges WHERE user_id = ?1 AND kind = 'password_reset'",
        [user_id],
    )?;
    Ok(())
}

/// Counts a failed sign-in. Returns the end of the lock it caused, if any.
/// Each lock lasts twice the previous one, up to the configured maximum.
///
/// # Errors
///
/// Database errors only.
pub fn record_failure(
    conn: &Connection,
    user_id: &str,
    now: i64,
    auth: &AuthConfig,
) -> Result<Option<i64>, DbError> {
    let (failed, locks): (i64, i64) = conn.query_row(
        "UPDATE users SET failed_logins = failed_logins + 1 WHERE id = ?1
         RETURNING failed_logins, lock_count",
        [user_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if failed < i64::from(auth.lockout_attempts) {
        return Ok(None);
    }
    let doubling = 1_i64 << locks.clamp(0, 20);
    let minutes =
        (i64::from(auth.lockout_minutes) * doubling).min(i64::from(auth.lockout_max_minutes));
    let until = now + minutes * MINUTE;
    conn.execute(
        "UPDATE users SET failed_logins = 0, lock_count = lock_count + 1, locked_until = ?2 WHERE id = ?1",
        params![user_id, until],
    )?;
    Ok(Some(until))
}

/// A good sign-in clears the failure count and the lock history.
///
/// # Errors
///
/// Database errors only.
pub fn record_success(conn: &Connection, user_id: &str) -> Result<(), DbError> {
    conn.execute(
        "UPDATE users SET failed_logins = 0, lock_count = 0, locked_until = NULL WHERE id = ?1",
        [user_id],
    )?;
    Ok(())
}

fn issue_tokens(
    conn: &Connection,
    device_id: &str,
    now: i64,
    auth: &AuthConfig,
) -> Result<Tokens, DbError> {
    let (access, access_hash) = token::new_token(Kind::Access);
    let (refresh, refresh_hash) = token::new_token(Kind::Refresh);
    let access_expires_at = now + i64::from(auth.access_token_minutes) * MINUTE;
    let refresh_expires_at = now + i64::from(auth.refresh_token_days) * DAY;
    conn.execute(
        "INSERT INTO access_tokens (hash, device_id, expires_at) VALUES (?1, ?2, ?3)",
        params![access_hash.as_slice(), device_id, access_expires_at],
    )?;
    conn.execute(
        "INSERT INTO refresh_tokens (hash, device_id, expires_at) VALUES (?1, ?2, ?3)",
        params![refresh_hash.as_slice(), device_id, refresh_expires_at],
    )?;
    Ok(Tokens {
        device_id: device_id.into(),
        access,
        access_expires_at,
        refresh,
        refresh_expires_at,
    })
}

/// Signs a new device in. The device it `replaces`, this browser's or app's
/// last sign-in, is signed out when it belongs to the same account.
///
/// # Errors
///
/// Database errors only.
pub fn create_session(
    conn: &Connection,
    user_id: &str,
    device: &DeviceInfo,
    now: i64,
    auth: &AuthConfig,
) -> Result<Tokens, DbError> {
    let device_id = new_id();
    let name: String = device.name.trim().chars().take(64).collect();
    let platform: String = device.platform.trim().chars().take(16).collect();
    conn.execute(
        "INSERT INTO devices (id, user_id, name, platform, created_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![device_id, user_id, name, platform, now],
    )?;
    if let Some(old) = device.replaces.as_deref() {
        let ours: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM devices WHERE id = ?1 AND user_id = ?2)",
            params![old, user_id],
            |r| r.get(0),
        )?;
        if ours {
            revoke_device(conn, old, now)?;
        }
    }
    issue_tokens(conn, &device_id, now, auth)
}

/// How long after a refresh the same token may come again, for an answer
/// lost on the way.
pub const REFRESH_RETRY_MS: i64 = 60_000;

/// Trades a refresh token for a new pair. Run in a transaction: the old
/// token is marked used and the new pair issued together.
///
/// # Errors
///
/// Database errors as `Err(Err(_))`, refusals as `Err(Ok(_))`.
pub fn rotate_refresh(
    conn: &Connection,
    hash: &TokenHash,
    now: i64,
    auth: &AuthConfig,
) -> Result<Result<(String, Tokens), RefreshRefused>, DbError> {
    let row: Option<RefreshRow> = conn
        .query_row(
            "SELECT d.id, d.user_id, t.expires_at, t.used_at, d.revoked_at
             FROM refresh_tokens t JOIN devices d ON d.id = t.device_id WHERE t.hash = ?1",
            [hash.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((device_id, user_id, expires_at, used_at, revoked_at)) = row else {
        return Ok(Err(RefreshRefused::Invalid));
    };
    if revoked_at.is_some() || expires_at <= now {
        return Ok(Err(RefreshRefused::Invalid));
    }
    if let Some(used_at) = used_at {
        // the answer to a refresh lost on the way (a cut connection): the
        // device sends the same token again. Shortly after and while the
        // pair it never got is unused, it gets a new pair and that one
        // stops working. Otherwise a copy of the token is in other hands:
        // the device is signed out, the copy with it.
        let pair_used: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM refresh_tokens WHERE device_id = ?1 AND used_at > ?2)",
            params![device_id, used_at],
            |r| r.get(0),
        )?;
        if now - used_at > REFRESH_RETRY_MS || pair_used {
            revoke_device(conn, &device_id, now)?;
            return Ok(Err(RefreshRefused::Reused { user_id, device_id }));
        }
        conn.execute(
            "UPDATE refresh_tokens SET used_at = ?2 WHERE device_id = ?1 AND used_at IS NULL",
            params![device_id, now],
        )?;
        return Ok(Ok((user_id, issue_tokens(conn, &device_id, now, auth)?)));
    }
    conn.execute(
        "UPDATE refresh_tokens SET used_at = ?2 WHERE hash = ?1",
        params![hash.as_slice(), now],
    )?;
    conn.execute(
        "UPDATE devices SET last_seen_at = ?2 WHERE id = ?1",
        params![device_id, now],
    )?;
    Ok(Ok((user_id, issue_tokens(conn, &device_id, now, auth)?)))
}

/// The account and device behind an access token, if it is valid.
///
/// # Errors
///
/// Database errors only.
pub fn authenticate(
    conn: &Connection,
    hash: &TokenHash,
    now: i64,
) -> Result<Option<Authed>, DbError> {
    let found: Option<(String, String, i64)> = conn
        .query_row(
            "SELECT d.id, d.user_id, d.last_seen_at FROM access_tokens t
             JOIN devices d ON d.id = t.device_id
             WHERE t.hash = ?1 AND t.expires_at > ?2 AND d.revoked_at IS NULL",
            params![hash.as_slice(), now],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((device_id, user_id, last_seen)) = found else {
        return Ok(None);
    };
    let Some(user) = user_by_id(conn, &user_id)? else {
        return Ok(None);
    };
    if user.disabled_at.is_some() {
        return Ok(None);
    }
    if now - last_seen > SEEN_EVERY {
        conn.execute(
            "UPDATE devices SET last_seen_at = ?2 WHERE id = ?1",
            params![device_id, now],
        )?;
    }
    Ok(Some(Authed { user, device_id }))
}

/// Signs a device out: its tokens stop working at once. Used refresh tokens
/// stay until they expire so a late copy is still recognised.
///
/// # Errors
///
/// Database errors only.
pub fn revoke_device(conn: &Connection, device_id: &str, now: i64) -> Result<(), DbError> {
    conn.execute(
        "UPDATE devices SET revoked_at = ?2 WHERE id = ?1 AND revoked_at IS NULL",
        params![device_id, now],
    )?;
    conn.execute(
        "DELETE FROM access_tokens WHERE device_id = ?1",
        [device_id],
    )?;
    conn.execute(
        "DELETE FROM refresh_tokens WHERE device_id = ?1 AND used_at IS NULL",
        [device_id],
    )?;
    Ok(())
}

/// Signs out every device of an account but one, `None` for all of them.
///
/// # Errors
///
/// Database errors only.
pub fn revoke_other_devices(
    conn: &Connection,
    user_id: &str,
    keep: Option<&str>,
    now: i64,
) -> Result<usize, DbError> {
    let ids: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT id FROM devices WHERE user_id = ?1 AND revoked_at IS NULL")?;
        stmt.query_map([user_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut count = 0;
    for id in ids.iter().filter(|id| Some(id.as_str()) != keep) {
        revoke_device(conn, id, now)?;
        count += 1;
    }
    Ok(count)
}

/// Signed in devices of an account, latest first.
///
/// # Errors
///
/// Database errors only.
pub fn list_devices(conn: &Connection, user_id: &str) -> Result<Vec<DeviceRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, platform, created_at, last_seen_at FROM devices
         WHERE user_id = ?1 AND revoked_at IS NULL ORDER BY last_seen_at DESC",
    )?;
    let rows = stmt.query_map([user_id], |r| {
        Ok(DeviceRow {
            id: r.get(0)?,
            name: r.get(1)?,
            platform: r.get(2)?,
            created_at: r.get(3)?,
            last_seen_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Whether this device belongs to the account and is signed in.
///
/// # Errors
///
/// Database errors only.
pub fn owns_device(conn: &Connection, user_id: &str, device_id: &str) -> Result<bool, DbError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM devices WHERE id = ?1 AND user_id = ?2 AND revoked_at IS NULL)",
        params![device_id, user_id],
        |r| r.get(0),
    )?)
}

/// Current registration mode.
///
/// # Errors
///
/// Database errors only.
pub fn registration(conn: &Connection) -> Result<Registration, DbError> {
    let value: String = conn.query_row(
        "SELECT value FROM settings WHERE key = 'registration'",
        [],
        |r| r.get(0),
    )?;
    Ok(match value.as_str() {
        "open" => Registration::Open,
        "invite" => Registration::Invite,
        _ => Registration::Closed,
    })
}

/// Changes the registration mode.
///
/// # Errors
///
/// Database errors only.
pub fn set_registration(conn: &Connection, mode: Registration) -> Result<(), DbError> {
    let value = match mode {
        Registration::Closed => "closed",
        Registration::Invite => "invite",
        Registration::Open => "open",
    };
    conn.execute(
        "UPDATE settings SET value = ?1 WHERE key = 'registration'",
        [value],
    )?;
    Ok(())
}

/// Creates an invite. Returns its id, plain token and expiry.
///
/// # Errors
///
/// Database errors only.
pub fn create_invite(
    conn: &Connection,
    created_by: &str,
    now: i64,
    life: i64,
) -> Result<(String, String, i64), DbError> {
    let (plain, hash) = token::new_token(Kind::Invite);
    let id = new_id();
    let expires_at = now + life;
    conn.execute(
        "INSERT INTO invites (id, hash, created_by, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, hash.as_slice(), created_by, now, expires_at],
    )?;
    Ok((id, plain, expires_at))
}

/// Marks a pending invite used. False when it cannot be used.
///
/// # Errors
///
/// Database errors only.
pub fn use_invite(
    conn: &Connection,
    hash: &TokenHash,
    user_id: &str,
    now: i64,
) -> Result<bool, DbError> {
    let changed = conn.execute(
        "UPDATE invites SET used_at = ?3, used_by = ?2
         WHERE hash = ?1 AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?3",
        params![hash.as_slice(), user_id, now],
    )?;
    Ok(changed == 1)
}

/// Whether an invite could be used now, checked before creating the account.
///
/// # Errors
///
/// Database errors only.
pub fn invite_usable(conn: &Connection, hash: &TokenHash, now: i64) -> Result<bool, DbError> {
    let creator: Option<Option<String>> = conn
        .query_row(
            "SELECT created_by FROM invites
             WHERE hash = ?1 AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?2",
            params![hash.as_slice(), now],
            |r| r.get(0),
        )
        .optional()?;
    let Some(creator) = creator else {
        return Ok(false);
    };
    // an invite lasts as long as who made it may let people in: their
    // account still here and on, and for a member, sign-up still open to
    // invites
    let Some(user) = creator
        .map(|id| user_by_id(conn, &id))
        .transpose()?
        .flatten()
    else {
        return Ok(false);
    };
    if user.disabled_at.is_some() {
        return Ok(false);
    }
    Ok(user.role == Role::Admin || registration(conn)? != Registration::Closed)
}

/// Invites, all of them or those one account created, latest first.
///
/// # Errors
///
/// Database errors only.
pub fn list_invites(
    conn: &Connection,
    created_by: Option<&str>,
    now: i64,
) -> Result<Vec<InviteRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT i.id, COALESCE(u.username, ''), i.created_at, i.expires_at, i.used_at, i.revoked_at
         FROM invites i LEFT JOIN users u ON u.id = i.created_by
         WHERE ?1 IS NULL OR i.created_by = ?1 ORDER BY i.created_at DESC",
    )?;
    let rows = stmt.query_map([created_by], |r| {
        let used: Option<i64> = r.get(4)?;
        let revoked: Option<i64> = r.get(5)?;
        let expires_at: i64 = r.get(3)?;
        Ok(InviteRow {
            id: r.get(0)?,
            created_by: r.get(1)?,
            created_at: r.get(2)?,
            expires_at,
            state: if used.is_some() {
                "used"
            } else if revoked.is_some() {
                "revoked"
            } else if expires_at <= now {
                "expired"
            } else {
                "pending"
            },
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Revokes a pending invite. With `created_by`, only one that account made.
/// False when there was nothing to revoke.
///
/// # Errors
///
/// Database errors only.
pub fn revoke_invite(
    conn: &Connection,
    id: &str,
    created_by: Option<&str>,
    now: i64,
) -> Result<bool, DbError> {
    let changed = conn.execute(
        "UPDATE invites SET revoked_at = ?3
         WHERE id = ?1 AND (?2 IS NULL OR created_by = ?2) AND used_at IS NULL AND revoked_at IS NULL",
        params![id, created_by, now],
    )?;
    Ok(changed == 1)
}

/// One security event in the audit log.
#[derive(Debug, Default)]
pub struct Event<'a> {
    /// Event name, for example `sign_in`.
    pub name: &'a str,
    /// Account concerned.
    pub user_id: Option<&'a str>,
    /// Device concerned.
    pub device_id: Option<&'a str>,
    /// Client address.
    pub ip: Option<&'a str>,
    /// Anything else worth keeping, never a secret.
    pub detail: Option<&'a str>,
}

/// Writes an event to the audit log.
///
/// # Errors
///
/// Database errors only.
pub fn audit(conn: &Connection, now: i64, event: &Event<'_>) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO audit_log (at, event, user_id, device_id, ip, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![now, event.name, event.user_id, event.device_id, event.ip, event.detail],
    )?;
    Ok(())
}

/// Deletes what can never work again: expired access tokens, and refresh
/// tokens past their expiry, used or not.
///
/// # Errors
///
/// Database errors only.
pub fn cleanup(conn: &Connection, now: i64) -> Result<usize, DbError> {
    let access = conn.execute("DELETE FROM access_tokens WHERE expires_at <= ?1", [now])?;
    let refresh = conn.execute("DELETE FROM refresh_tokens WHERE expires_at <= ?1", [now])?;
    let challenges = super::factors::cleanup(conn, now)?;
    Ok(access + refresh + challenges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn auth() -> AuthConfig {
        AuthConfig::default()
    }

    fn device() -> DeviceInfo {
        DeviceInfo {
            name: "Pixel 7".into(),
            platform: "android".into(),
            replaces: None,
        }
    }

    async fn with_user() -> (Db, UserRow) {
        let db = Db::in_memory().unwrap();
        let user = db
            .call(|c| {
                Ok(insert_user(c, "anna", "Anna", "$argon2id$x", Role::User, 1_000)?.unwrap())
            })
            .await
            .unwrap();
        (db, user)
    }

    #[tokio::test]
    async fn a_sign_in_replaces_its_own_last_one_only() {
        let (db, anna) = with_user().await;
        let devices = db.clone();
        let list = |user: String| {
            let db = devices.clone();
            async move {
                db.call(move |c| list_devices(c, &user))
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|d| d.id)
                    .collect::<Vec<_>>()
            }
        };
        let (id, bob) = (
            anna.id.clone(),
            db.call(|c| {
                Ok(insert_user(c, "bob", "Bob", "$argon2id$x", Role::User, 1_000)?.unwrap())
            })
            .await
            .unwrap(),
        );
        let first = db
            .call(move |c| create_session(c, &id, &device(), 1_000, &auth()))
            .await
            .unwrap();
        let bob_id = bob.id.clone();
        let bobs = db
            .call(move |c| create_session(c, &bob_id, &device(), 1_000, &auth()))
            .await
            .unwrap();
        // the same browser again, naming its last sign-in
        let (id, old) = (anna.id.clone(), first.device_id.clone());
        let second = db
            .call(move |c| {
                let device = DeviceInfo {
                    replaces: Some(old),
                    ..device()
                };
                create_session(c, &id, &device, 2_000, &auth())
            })
            .await
            .unwrap();
        assert_eq!(list(anna.id.clone()).await, vec![second.device_id.clone()]);
        // another account's device is left alone
        let (id, other) = (anna.id.clone(), bobs.device_id.clone());
        let third = db
            .call(move |c| {
                let device = DeviceInfo {
                    replaces: Some(other),
                    ..device()
                };
                create_session(c, &id, &device, 3_000, &auth())
            })
            .await
            .unwrap();
        assert_eq!(list(bob.id.clone()).await, vec![bobs.device_id]);
        assert_eq!(list(anna.id).await.len(), 2, "{}", third.device_id);
    }

    #[tokio::test]
    async fn usernames_are_unique_whatever_the_case() {
        let (db, _) = with_user().await;
        let again = db
            .call(|c| insert_user(c, "ANNA", "x", "h", Role::User, 2_000))
            .await
            .unwrap();
        assert!(again.is_none());
    }

    #[tokio::test]
    async fn a_refresh_whose_answer_was_lost_can_be_sent_again_shortly_after() {
        let (db, user) = with_user().await;
        let id = user.id.clone();
        let first = db
            .call(move |c| create_session(c, &id, &device(), 1_000, &auth()))
            .await
            .unwrap();
        let rotate = |hash: TokenHash, at: i64| {
            let db = db.clone();
            async move {
                db.call(move |c| rotate_refresh(c, &hash, at, &auth()))
                    .await
                    .unwrap()
            }
        };
        let old = token::hash(&first.refresh);
        let (_, lost) = rotate(old, 2_000).await.unwrap();
        let (_, again) = rotate(old, 2_500).await.unwrap();
        assert_ne!(again.refresh, lost.refresh);
        // the pair that never arrived is no good, the one that did works
        assert!(matches!(
            rotate(token::hash(&lost.refresh), 2_600 + REFRESH_RETRY_MS).await,
            Err(RefreshRefused::Reused { .. } | RefreshRefused::Invalid)
        ));
    }

    #[tokio::test]
    async fn the_retry_window_ends_once_the_new_pair_is_used() {
        let (db, user) = with_user().await;
        let id = user.id.clone();
        let first = db
            .call(move |c| create_session(c, &id, &device(), 1_000, &auth()))
            .await
            .unwrap();
        let rotate = |hash: TokenHash, at: i64| {
            let db = db.clone();
            async move {
                db.call(move |c| rotate_refresh(c, &hash, at, &auth()))
                    .await
                    .unwrap()
            }
        };
        let old = token::hash(&first.refresh);
        let (_, second) = rotate(old, 2_000).await.unwrap();
        rotate(token::hash(&second.refresh), 2_100).await.unwrap();
        assert!(matches!(
            rotate(old, 2_200).await,
            Err(RefreshRefused::Reused { .. })
        ));
    }

    #[tokio::test]
    async fn a_refresh_token_works_once_and_a_second_use_signs_the_device_out() {
        let (db, user) = with_user().await;
        let id = user.id.clone();
        let first = db
            .call(move |c| create_session(c, &id, &device(), 1_000, &auth()))
            .await
            .unwrap();
        let old = token::hash(&first.refresh);
        let (_, second) = db
            .call(move |c| rotate_refresh(c, &old, 2_000, &auth()))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(second.refresh, first.refresh);

        // the stolen copy comes back, after the retry window
        let old = token::hash(&first.refresh);
        let refused = db
            .call(move |c| rotate_refresh(c, &old, 3_000 + REFRESH_RETRY_MS, &auth()))
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(refused, RefreshRefused::Reused { .. }));
        // and the legitimate new tokens died with the device
        let new_refresh = token::hash(&second.refresh);
        let new_access = token::hash(&second.access);
        let after = db
            .call(move |c| rotate_refresh(c, &new_refresh, 4_000 + REFRESH_RETRY_MS, &auth()))
            .await
            .unwrap();
        assert_eq!(after.unwrap_err(), RefreshRefused::Invalid);
        assert!(
            db.call(move |c| authenticate(c, &new_access, 4_000 + REFRESH_RETRY_MS))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn tokens_expire() {
        let (db, user) = with_user().await;
        let id = user.id.clone();
        let tokens = db
            .call(move |c| create_session(c, &id, &device(), 0, &auth()))
            .await
            .unwrap();
        let access = token::hash(&tokens.access);
        assert!(
            db.call(move |c| authenticate(c, &access, 14 * MINUTE))
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            db.call(move |c| authenticate(c, &access, 16 * MINUTE))
                .await
                .unwrap()
                .is_none()
        );
        let refresh = token::hash(&tokens.refresh);
        let late = db
            .call(move |c| rotate_refresh(c, &refresh, 61 * DAY, &auth()))
            .await
            .unwrap();
        assert_eq!(late.unwrap_err(), RefreshRefused::Invalid);
    }

    #[tokio::test]
    async fn failures_lock_with_doubling_waits_and_a_good_sign_in_resets() {
        let (db, user) = with_user().await;
        let id = user.id.clone();
        let locks = db
            .call(move |c| {
                let mut ends = Vec::new();
                for round in 0..3 {
                    let now = round * 10 * MINUTE;
                    for _ in 0..5 {
                        if let Some(until) = record_failure(c, &id, now, &auth())? {
                            ends.push((until - now) / MINUTE);
                        }
                    }
                }
                record_success(c, &id)?;
                ends.push(record_failure(c, &id, 0, &auth())?.unwrap_or(-1));
                Ok(ends)
            })
            .await
            .unwrap();
        assert_eq!(
            locks,
            vec![1, 2, 4, -1],
            "1, 2 then 4 minutes, and no lock after a reset"
        );
    }

    #[tokio::test]
    async fn invites_work_once_until_they_expire_or_are_revoked() {
        let (db, user) = with_user().await;
        let id = user.id.clone();
        db.call(move |c| {
            let (_, plain, _) = create_invite(c, &id, 0, 7 * DAY)?;
            let hash = token::hash(&plain);
            // A member's invite needs sign-up
            // open to invites, and its maker's account on
            assert!(!invite_usable(c, &hash, DAY)?, "sign-up closed");
            set_registration(c, Registration::Invite)?;
            assert!(invite_usable(c, &hash, DAY)?);
            c.execute("UPDATE users SET disabled_at = 1 WHERE id = ?1", [&id])?;
            assert!(!invite_usable(c, &hash, DAY)?, "its maker turned off");
            c.execute("UPDATE users SET disabled_at = NULL WHERE id = ?1", [&id])?;
            assert!(use_invite(c, &hash, &id, DAY)?);
            assert!(!use_invite(c, &hash, &id, DAY)?, "single use");

            let (late_id, late, _) = create_invite(c, &id, 0, DAY)?;
            assert!(!invite_usable(c, &token::hash(&late), 2 * DAY)?, "expired");
            assert!(revoke_invite(c, &late_id, Some(&id), 0)?);

            let states: Vec<&str> = list_invites(c, None, 2 * DAY)?
                .iter()
                .map(|i| i.state)
                .collect();
            assert_eq!(states.len(), 2);
            assert!(states.contains(&"used") && states.contains(&"revoked"));
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn registration_defaults_to_closed() {
        let db = Db::in_memory().unwrap();
        let (before, after) = db
            .call(|c| {
                let before = registration(c)?;
                set_registration(c, Registration::Open)?;
                Ok((before, registration(c)?))
            })
            .await
            .unwrap();
        assert_eq!(before, Registration::Closed);
        assert_eq!(after, Registration::Open);
    }
}
