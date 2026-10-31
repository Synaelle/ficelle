// SPDX-License-Identifier: AGPL-3.0-or-later

//! Storage for second factors and for challenges, the single use tokens
//! that carry a sign-in or a passkey ceremony from one request to the next.

use base64::Engine;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use webauthn_rs::prelude::Passkey;

use super::token::{self, Kind, TokenHash};
use crate::db::DbError;
use crate::keys::MasterKey;

/// What a challenge is for. A token made for one kind never works for another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeKind {
    /// Password checked, waiting for the second factor.
    SecondFactor,
    /// A passkey being added to a signed in account.
    PasskeyAdd,
    /// Signing in with a passkey alone.
    PasskeySignIn,
    /// Gone to the single sign-on provider, waiting for its callback.
    SsoStart,
    /// Back from the provider, the page trades it for a session.
    SsoGrant,
    /// An OPAQUE sign-in between its two requests.
    OpaqueLogin,
    /// An OPAQUE run proving the password of a signed in account.
    OpaqueConfirm,
    /// The proof it gives, spent by one sensitive change.
    ConfirmProof,
    /// A recovery code checked, waiting for the new password.
    Recovery,
    /// A link sent to an address, proving it when opened.
    EmailVerify,
    /// A password reset link sent by email.
    PasswordReset,
}

impl ChallengeKind {
    fn text(self) -> &'static str {
        match self {
            Self::SecondFactor => "second_factor",
            Self::PasskeyAdd => "passkey_add",
            Self::PasskeySignIn => "passkey_sign_in",
            Self::SsoStart => "sso_start",
            Self::SsoGrant => "sso_grant",
            Self::OpaqueLogin => "opaque_login",
            Self::OpaqueConfirm => "opaque_confirm",
            Self::ConfirmProof => "confirm_proof",
            Self::Recovery => "recovery",
            Self::EmailVerify => "email_verify",
            Self::PasswordReset => "password_reset",
        }
    }
}

/// A live challenge.
#[derive(Debug, Clone)]
pub struct Challenge<T> {
    /// Account it belongs to, if any yet.
    pub user_id: Option<String>,
    /// What was kept for the next step.
    pub data: T,
    /// Wrong answers so far.
    pub attempts: i64,
}

/// Authenticator app state of an account.
#[derive(Debug, Clone, Default)]
pub struct TotpState {
    /// Active secret.
    pub secret: Option<String>,
    /// Secret waiting for its first code.
    pub pending: Option<String>,
    /// Last step accepted.
    pub last_step: Option<i64>,
}

/// Stores a challenge and returns its token.
///
/// # Errors
///
/// Database errors, or data that does not serialise.
pub fn create_challenge<T: Serialize>(
    conn: &Connection,
    kind: ChallengeKind,
    user_id: Option<&str>,
    data: &T,
    expires_at: i64,
) -> Result<String, DbError> {
    let (token, hash) = token::new_token(Kind::Challenge);
    store_challenge(conn, &hash, kind, user_id, data, expires_at)?;
    Ok(token)
}

/// Stores a challenge for a token made beforehand, for a token that has to
/// travel before the data exists (the state of a single sign-on).
///
/// # Errors
///
/// Database errors, or data that does not serialise.
pub fn store_challenge<T: Serialize>(
    conn: &Connection,
    hash: &TokenHash,
    kind: ChallengeKind,
    user_id: Option<&str>,
    data: &T,
    expires_at: i64,
) -> Result<(), DbError> {
    let json = serde_json::to_string(data).map_err(|e| DbError::Data(e.to_string()))?;
    conn.execute(
        "INSERT INTO challenges (hash, kind, user_id, data, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![hash.as_slice(), kind.text(), user_id, json, expires_at],
    )?;
    Ok(())
}

/// The challenge behind a token, if it is of this kind and not expired.
///
/// # Errors
///
/// Database errors, or stored data of another shape.
pub fn challenge<T: DeserializeOwned>(
    conn: &Connection,
    hash: &TokenHash,
    kind: ChallengeKind,
    now: i64,
) -> Result<Option<Challenge<T>>, DbError> {
    let row: Option<(Option<String>, String, i64)> = conn
        .query_row(
            "SELECT user_id, data, attempts FROM challenges
             WHERE hash = ?1 AND kind = ?2 AND expires_at > ?3",
            params![hash.as_slice(), kind.text(), now],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((user_id, json, attempts)) = row else {
        return Ok(None);
    };
    let data = serde_json::from_str(&json).map_err(|e| DbError::Data(e.to_string()))?;
    Ok(Some(Challenge {
        user_id,
        data,
        attempts,
    }))
}

/// Counts a wrong answer, returns the count.
///
/// # Errors
///
/// Database errors only.
pub fn challenge_failed(conn: &Connection, hash: &TokenHash) -> Result<i64, DbError> {
    Ok(conn
        .query_row(
            "UPDATE challenges SET attempts = attempts + 1 WHERE hash = ?1 RETURNING attempts",
            [hash.as_slice()],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(i64::MAX))
}

/// Ends a challenge. `false` when it was already gone, which means another
/// request used it first.
///
/// # Errors
///
/// Database errors only.
pub fn end_challenge(conn: &Connection, hash: &TokenHash) -> Result<bool, DbError> {
    Ok(conn.execute("DELETE FROM challenges WHERE hash = ?1", [hash.as_slice()])? == 1)
}

/// Authenticator app state.
///
/// # Errors
///
/// Database errors only.
pub fn totp(conn: &Connection, user_id: &str) -> Result<TotpState, DbError> {
    Ok(conn
        .query_row(
            "SELECT totp_secret, totp_pending, totp_last_step FROM users WHERE id = ?1",
            [user_id],
            |r| {
                Ok(TotpState {
                    secret: r.get(0)?,
                    pending: r.get(1)?,
                    last_step: r.get(2)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

/// Keeps a new secret aside until a first code confirms it, sealed by the
/// master key: a copy of the database alone makes no codes.
///
/// # Errors
///
/// Database errors only.
pub fn set_totp_pending(
    conn: &Connection,
    master: &MasterKey,
    user_id: &str,
    secret: &str,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE users SET totp_pending = ?2 WHERE id = ?1",
        params![user_id, seal_totp(master, user_id, secret)],
    )?;
    Ok(())
}

/// Marks a sealed secret in its column.
const SEALED: &str = "sealed:";

fn seal_totp(master: &MasterKey, user_id: &str, secret: &str) -> String {
    let sealed = master.seal_setting(&format!("totp:{user_id}"), secret.as_bytes());
    format!(
        "{SEALED}{}",
        base64::engine::general_purpose::STANDARD.encode(sealed)
    )
}

/// A stored secret as the codes need it: opened when sealed, as it is when
/// written before 0.3.0. `None` when it does not open with this key.
pub fn open_totp(master: &MasterKey, user_id: &str, stored: &str) -> Option<String> {
    let Some(sealed) = stored.strip_prefix(SEALED) else {
        return Some(stored.to_owned());
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(sealed)
        .ok()?;
    let opened = master.open_setting(&format!("totp:{user_id}"), &bytes)?;
    String::from_utf8(opened.to_vec()).ok()
}

/// Seals the secrets stored plain before 0.3.0, at start. Returns how many.
///
/// # Errors
///
/// Database errors only.
pub fn seal_plain_totp(conn: &Connection, master: &MasterKey) -> Result<usize, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, totp_secret, totp_pending FROM users
         WHERE (totp_secret IS NOT NULL AND totp_secret NOT LIKE 'sealed:%')
            OR (totp_pending IS NOT NULL AND totp_pending NOT LIKE 'sealed:%')",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, secret, pending) in &rows {
        let sealed = |s: &Option<String>| {
            s.as_ref().map(|s| {
                if s.starts_with(SEALED) {
                    s.clone()
                } else {
                    seal_totp(master, id, s)
                }
            })
        };
        conn.execute(
            "UPDATE users SET totp_secret = ?2, totp_pending = ?3 WHERE id = ?1",
            params![id, sealed(secret), sealed(pending)],
        )?;
    }
    Ok(rows.len())
}

/// Turns the pending secret on, `step` being the confirming code's.
///
/// # Errors
///
/// Database errors only.
pub fn activate_totp(conn: &Connection, user_id: &str, step: i64) -> Result<(), DbError> {
    conn.execute(
        "UPDATE users SET totp_secret = totp_pending, totp_pending = NULL, totp_last_step = ?2
         WHERE id = ?1 AND totp_pending IS NOT NULL",
        params![user_id, step],
    )?;
    Ok(())
}

/// Remembers the step of a code just used.
///
/// # Errors
///
/// Database errors only.
pub fn used_totp_step(conn: &Connection, user_id: &str, step: i64) -> Result<(), DbError> {
    conn.execute(
        "UPDATE users SET totp_last_step = ?2 WHERE id = ?1",
        params![user_id, step],
    )?;
    Ok(())
}

/// Turns the authenticator app off.
///
/// # Errors
///
/// Database errors only.
pub fn disable_totp(conn: &Connection, user_id: &str) -> Result<(), DbError> {
    conn.execute(
        "UPDATE users SET totp_secret = NULL, totp_pending = NULL, totp_last_step = NULL WHERE id = ?1",
        [user_id],
    )?;
    guard_admin(conn, user_id)
}

/// Replaces every recovery code of the account.
///
/// # Errors
///
/// Database errors only.
pub fn replace_recovery_codes(
    conn: &Connection,
    user_id: &str,
    hashes: &[TokenHash],
) -> Result<(), DbError> {
    conn.execute("DELETE FROM recovery_codes WHERE user_id = ?1", [user_id])?;
    for hash in hashes {
        conn.execute(
            "INSERT OR IGNORE INTO recovery_codes (user_id, hash) VALUES (?1, ?2)",
            params![user_id, hash.as_slice()],
        )?;
    }
    Ok(())
}

/// Uses a recovery code: `true` when it was unused and is now spent.
///
/// # Errors
///
/// Database errors only.
pub fn use_recovery_code(
    conn: &Connection,
    user_id: &str,
    hash: &TokenHash,
    now: i64,
) -> Result<bool, DbError> {
    Ok(conn.execute(
        "UPDATE recovery_codes SET used_at = ?3 WHERE user_id = ?1 AND hash = ?2 AND used_at IS NULL",
        params![user_id, hash.as_slice(), now],
    )? == 1)
}

/// Recovery codes not used yet.
///
/// # Errors
///
/// Database errors only.
pub fn recovery_codes_left(conn: &Connection, user_id: &str) -> Result<u32, DbError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM recovery_codes WHERE user_id = ?1 AND used_at IS NULL",
        [user_id],
        |r| r.get(0),
    )?;
    Ok(u32::try_from(n).unwrap_or(0))
}

/// Whether the account asks for a second factor at sign-in: an
/// authenticator app, a passkey, or codes by email to a confirmed address.
///
/// # Errors
///
/// Database errors only.
pub fn has_second_factor(conn: &Connection, user_id: &str) -> Result<bool, DbError> {
    Ok(totp(conn, user_id)?.secret.is_some()
        || !passkeys(conn, user_id)?.is_empty()
        || email_codes(conn, user_id)?.is_some())
}

/// Whether admins must have a second step to act as admin. On unless the
/// admin who set the server up chose otherwise, or turned it off later.
///
/// # Errors
///
/// Database errors only.
pub fn admins_need_second_step(conn: &Connection) -> Result<bool, DbError> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'admin_second_step'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.as_deref() != Some("optional"))
}

/// Sets whether admins must have a second step. On, every admin without
/// one stops acting as admin until they add one; off, they all act again.
///
/// # Errors
///
/// Database errors only.
pub fn set_admins_need_second_step(conn: &Connection, needed: bool) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('admin_second_step', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [if needed { "required" } else { "optional" }],
    )?;
    if needed {
        let admins: Vec<String> = conn
            .prepare("SELECT id FROM users WHERE role = 'admin'")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in admins {
            guard_admin(conn, &id)?;
        }
    } else {
        conn.execute("UPDATE users SET factor_required = 0", [])?;
    }
    Ok(())
}

/// An admin left without a second factor acts as admin again only once
/// they add one, unless the server lets
/// admins go without ([`admins_need_second_step`]). Called wherever an
/// account becomes admin or loses a factor; adding a factor clears the need.
///
/// # Errors
///
/// Database errors only.
pub fn guard_admin(conn: &Connection, user_id: &str) -> Result<(), DbError> {
    if admins_need_second_step(conn)? && !has_second_factor(conn, user_id)? {
        conn.execute(
            "UPDATE users SET factor_required = 1 WHERE id = ?1 AND role = 'admin'",
            [user_id],
        )?;
    }
    Ok(())
}

/// The address codes go to when the account takes codes by email as its
/// second step and its address is confirmed. `None` otherwise.
///
/// # Errors
///
/// Database errors only.
pub fn email_codes(conn: &Connection, user_id: &str) -> Result<Option<String>, DbError> {
    Ok(conn
        .query_row(
            "SELECT email FROM users WHERE id = ?1 AND email_codes = 1
             AND email IS NOT NULL AND email_verified_at IS NOT NULL",
            [user_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
}

/// Turns codes by email on or off for the account.
///
/// # Errors
///
/// Database errors only.
pub fn set_email_codes(conn: &Connection, user_id: &str, on: bool) -> Result<(), DbError> {
    conn.execute(
        "UPDATE users SET email_codes = ?2 WHERE id = ?1",
        params![user_id, on],
    )?;
    if on {
        conn.execute(
            "UPDATE users SET factor_required = 0 WHERE id = ?1",
            [user_id],
        )?;
    } else {
        drop_unneeded_recovery_codes(conn, user_id)?;
        guard_admin(conn, user_id)?;
    }
    Ok(())
}

/// How many codes one sign-in may ask to be sent.
pub const EMAIL_SENDS: i64 = 3;

/// Keeps the code sent for the sign-in whose challenge is `challenge`,
/// replacing an earlier one. `false` when that sign-in already asked for
/// [`EMAIL_SENDS`] codes.
///
/// # Errors
///
/// Database errors only.
pub fn keep_email_code(
    conn: &Connection,
    challenge: &TokenHash,
    code: &str,
    expires_at: i64,
) -> Result<bool, DbError> {
    let sends: Option<i64> = conn
        .query_row(
            "SELECT sends FROM email_codes WHERE challenge = ?1",
            [challenge.as_slice()],
            |r| r.get(0),
        )
        .optional()?;
    if sends.is_some_and(|n| n >= EMAIL_SENDS) {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO email_codes (challenge, code, expires_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (challenge) DO UPDATE SET code = excluded.code,
             expires_at = excluded.expires_at, sends = sends + 1",
        params![
            challenge.as_slice(),
            token::hash(code).as_slice(),
            expires_at
        ],
    )?;
    Ok(true)
}

/// Whether `code` is the one sent for this sign-in and still fresh. A
/// right code is spent.
///
/// # Errors
///
/// Database errors only.
pub fn use_email_code(
    conn: &Connection,
    challenge: &TokenHash,
    code: &str,
    now: i64,
) -> Result<bool, DbError> {
    let stored: Option<Vec<u8>> = conn
        .query_row(
            "SELECT code FROM email_codes WHERE challenge = ?1 AND expires_at > ?2",
            params![challenge.as_slice(), now],
            |r| r.get(0),
        )
        .optional()?;
    // both sides are sha-256 outputs, the comparison tells nothing useful
    let right = stored.is_some_and(|s| s.as_slice() == token::hash(code.trim()).as_slice());
    if right {
        conn.execute(
            "DELETE FROM email_codes WHERE challenge = ?1",
            [challenge.as_slice()],
        )?;
    }
    Ok(right)
}

/// Recovery codes only stand in for a second factor: without one left,
/// they go.
///
/// # Errors
///
/// Database errors only.
pub fn drop_unneeded_recovery_codes(conn: &Connection, user_id: &str) -> Result<(), DbError> {
    if !has_second_factor(conn, user_id)? {
        conn.execute("DELETE FROM recovery_codes WHERE user_id = ?1", [user_id])?;
    }
    Ok(())
}

/// When a device signed in.
///
/// # Errors
///
/// Database errors only.
pub fn device_signed_in_at(conn: &Connection, device_id: &str) -> Result<Option<i64>, DbError> {
    Ok(conn
        .query_row(
            "SELECT created_at FROM devices WHERE id = ?1",
            [device_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// A stored passkey.
#[derive(Debug, Clone)]
pub struct PasskeyRow {
    /// Id.
    pub id: String,
    /// Name given by its owner.
    pub name: String,
    /// The credential, for webauthn-rs.
    pub passkey: Passkey,
    /// When it was added.
    pub created_at: i64,
    /// Last sign-in with it.
    pub last_used_at: Option<i64>,
}

/// Every passkey of an account, oldest first.
///
/// # Errors
///
/// Database errors, or a credential that no longer reads.
pub fn passkeys(conn: &Connection, user_id: &str) -> Result<Vec<PasskeyRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, credential, created_at, last_used_at FROM passkeys
         WHERE user_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map([user_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, Option<i64>>(4)?,
        ))
    })?;
    let mut list = Vec::new();
    for row in rows {
        let (id, name, json, created_at, last_used_at) = row?;
        let passkey = serde_json::from_str(&json).map_err(|e| DbError::Data(e.to_string()))?;
        list.push(PasskeyRow {
            id,
            name,
            passkey,
            created_at,
            last_used_at,
        });
    }
    Ok(list)
}

/// Stores a new passkey. `None` when the same authenticator credential is
/// already registered, to this or another account.
///
/// # Errors
///
/// Database errors only.
pub fn insert_passkey(
    conn: &Connection,
    user_id: &str,
    name: &str,
    passkey: &Passkey,
    now: i64,
) -> Result<Option<String>, DbError> {
    let id = uuid::Uuid::now_v7().to_string();
    let json = serde_json::to_string(passkey).map_err(|e| DbError::Data(e.to_string()))?;
    let credential_id: &[u8] = passkey.cred_id().as_ref();
    let added = conn.execute(
        "INSERT OR IGNORE INTO passkeys (id, user_id, name, credential_id, credential, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, user_id, name, credential_id, json, now],
    )?;
    Ok((added == 1).then_some(id))
}

/// Saves a passkey after a sign-in: its counter may have moved.
///
/// # Errors
///
/// Database errors only.
pub fn passkey_used(conn: &Connection, passkey: &Passkey, now: i64) -> Result<(), DbError> {
    let json = serde_json::to_string(passkey).map_err(|e| DbError::Data(e.to_string()))?;
    let credential_id: &[u8] = passkey.cred_id().as_ref();
    conn.execute(
        "UPDATE passkeys SET credential = ?2, last_used_at = ?3 WHERE credential_id = ?1",
        params![credential_id, json, now],
    )?;
    Ok(())
}

/// Removes a passkey of the account, `false` when it has no such passkey.
///
/// # Errors
///
/// Database errors only.
pub fn delete_passkey(conn: &Connection, user_id: &str, id: &str) -> Result<bool, DbError> {
    let deleted = conn.execute(
        "DELETE FROM passkeys WHERE id = ?1 AND user_id = ?2",
        params![id, user_id],
    )? == 1;
    guard_admin(conn, user_id)?;
    Ok(deleted)
}

/// A linked single sign-on identity.
#[derive(Debug, Clone)]
pub struct SsoRow {
    /// Id.
    pub id: String,
    /// What to show for it.
    pub name: String,
    /// When it was linked.
    pub created_at: i64,
}

/// The account linked to a provider identity.
///
/// # Errors
///
/// Database errors only.
pub fn sso_user(conn: &Connection, issuer: &str, subject: &str) -> Result<Option<String>, DbError> {
    Ok(conn
        .query_row(
            "SELECT user_id FROM sso_identities WHERE issuer = ?1 AND subject = ?2",
            params![issuer, subject],
            |r| r.get(0),
        )
        .optional()?)
}

/// Links an identity to an account. `false` when it is linked already.
///
/// # Errors
///
/// Database errors only.
pub fn link_sso(
    conn: &Connection,
    user_id: &str,
    issuer: &str,
    subject: &str,
    name: &str,
    now: i64,
) -> Result<bool, DbError> {
    Ok(conn.execute(
        "INSERT OR IGNORE INTO sso_identities (id, user_id, issuer, subject, name, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            uuid::Uuid::now_v7().to_string(),
            user_id,
            issuer,
            subject,
            name,
            now
        ],
    )? == 1)
}

/// The identities linked to an account.
///
/// # Errors
///
/// Database errors only.
pub fn sso_identities(conn: &Connection, user_id: &str) -> Result<Vec<SsoRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at FROM sso_identities WHERE user_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map([user_id], |r| {
        Ok(SsoRow {
            id: r.get(0)?,
            name: r.get(1)?,
            created_at: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Unlinks an identity of the account, `false` when it has no such identity.
///
/// # Errors
///
/// Database errors only.
pub fn unlink_sso(conn: &Connection, user_id: &str, id: &str) -> Result<bool, DbError> {
    Ok(conn.execute(
        "DELETE FROM sso_identities WHERE id = ?1 AND user_id = ?2",
        params![id, user_id],
    )? == 1)
}

/// Removes every second factor: the app, the recovery codes, the passkeys.
///
/// # Errors
///
/// Database errors only.
pub fn reset_second_factors(conn: &Connection, user_id: &str) -> Result<(), DbError> {
    disable_totp(conn, user_id)?;
    conn.execute("UPDATE users SET email_codes = 0 WHERE id = ?1", [user_id])?;
    conn.execute("DELETE FROM recovery_codes WHERE user_id = ?1", [user_id])?;
    conn.execute("DELETE FROM passkeys WHERE user_id = ?1", [user_id])?;
    guard_admin(conn, user_id)
}

/// Deletes expired challenges.
///
/// # Errors
///
/// Database errors only.
pub fn cleanup(conn: &Connection, now: i64) -> Result<usize, DbError> {
    conn.execute("DELETE FROM email_codes WHERE expires_at <= ?1", [now])?;
    Ok(conn.execute("DELETE FROM challenges WHERE expires_at <= ?1", [now])?)
}

#[cfg(test)]
mod sealed_tests {
    use app_protocol::account::Role;

    use super::*;
    use crate::accounts::store;
    use crate::db::Db;

    /// Authenticator secrets are not in the
    /// database as they are, and those from before are sealed at start.
    #[tokio::test]
    async fn authenticator_secrets_are_sealed_by_the_master_key() {
        let db = Db::in_memory().unwrap();
        let master = std::sync::Arc::new(MasterKey::generate());
        let m = master.clone();
        let (raw, opened, old_raw, old_opened) = db
            .call(move |c| {
                let anna = store::insert_user(c, "anna", "Anna", "h", Role::User, 1)?.unwrap();
                set_totp_pending(c, &m, &anna.id, "JBSWY3DPEHPK3PXP")?;
                activate_totp(c, &anna.id, 1)?;
                let raw = totp(c, &anna.id)?.secret.unwrap();
                let opened = open_totp(&m, &anna.id, &raw);
                // as 0.2 kept it
                let bob = store::insert_user(c, "bob", "Bob", "h", Role::User, 1)?.unwrap();
                c.execute(
                    "UPDATE users SET totp_secret = 'KRSXG5CTMVRXEZLU' WHERE id = ?1",
                    [&bob.id],
                )?;
                assert_eq!(seal_plain_totp(c, &m)?, 1);
                assert_eq!(seal_plain_totp(c, &m)?, 0, "sealed once");
                let old_raw = totp(c, &bob.id)?.secret.unwrap();
                let old_opened = open_totp(&m, &bob.id, &old_raw);
                Ok((raw, opened, old_raw, old_opened))
            })
            .await
            .unwrap();
        assert!(raw.starts_with(SEALED) && !raw.contains("JBSWY3DPEHPK3PXP"));
        assert_eq!(opened.as_deref(), Some("JBSWY3DPEHPK3PXP"));
        assert!(old_raw.starts_with(SEALED));
        assert_eq!(old_opened.as_deref(), Some("KRSXG5CTMVRXEZLU"));
        assert!(open_totp(&MasterKey::generate(), "x", &raw).is_none());
    }
}
