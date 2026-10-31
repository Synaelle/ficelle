// SPDX-License-Identifier: AGPL-3.0-or-later

//! Storage for end-to-end encrypted accounts: the OPAQUE setup of the
//! server, each account's record and wrapped keys, and the policy on which
//! kinds sign-up creates.

use app_e2ee::server::Setup;
use app_protocol::e2ee::{AccountKind, AccountKinds, KeyBundle};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::db::DbError;
use crate::keys::MasterKey;

/// The kind from storage.
pub fn kind_of(text: &str) -> AccountKind {
    if text == "e2ee" {
        AccountKind::E2ee
    } else {
        AccountKind::Standard
    }
}

/// The server's OPAQUE setup, made and stored on first use. It lives in
/// the database because the records are useless without it, and a backup
/// must carry both. It is sealed by the master key: a copy of the database
/// alone cannot try passwords against the records offline. A setup stored
/// plain before 0.3.0 is sealed when
/// read.
///
/// # Errors
///
/// Database errors, or a stored setup that no longer reads.
pub fn setup(conn: &Connection, master: &MasterKey) -> Result<Setup, DbError> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM instance WHERE key = 'opaque_setup'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let unreadable = |e: &dyn std::fmt::Display| DbError::Data(e.to_string());
    if let Some(text) = stored {
        if let Some(sealed) = text.strip_prefix(SEALED) {
            let bytes = STANDARD.decode(sealed).map_err(|e| unreadable(&e))?;
            let opened = master.open_setting("opaque_setup", &bytes).ok_or_else(|| {
                unreadable(&"the OPAQUE setup does not open with this master key")
            })?;
            return Setup::from_bytes(&opened).map_err(|e| unreadable(&e));
        }
        let bytes = STANDARD.decode(text).map_err(|e| unreadable(&e))?;
        let setup = Setup::from_bytes(&bytes).map_err(|e| unreadable(&e))?;
        store_setup(conn, master, &setup)?;
        return Ok(setup);
    }
    let setup = Setup::generate();
    store_setup(conn, master, &setup)?;
    Ok(setup)
}

/// Marks a sealed setup in the `instance` table.
const SEALED: &str = "sealed:";

fn store_setup(conn: &Connection, master: &MasterKey, setup: &Setup) -> Result<(), DbError> {
    let sealed = master.seal_setting("opaque_setup", setup.to_bytes().as_slice());
    conn.execute(
        "INSERT INTO instance (key, value) VALUES ('opaque_setup', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [format!("{SEALED}{}", STANDARD.encode(sealed))],
    )?;
    Ok(())
}

/// What sign-up creates.
///
/// # Errors
///
/// Database errors only.
pub fn account_kinds(conn: &Connection) -> Result<AccountKinds, DbError> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'account_kinds'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match value.as_deref() {
        Some("standard") => AccountKinds::Standard,
        Some("e2ee") => AccountKinds::E2ee,
        _ => AccountKinds::Choice,
    })
}

/// Changes what sign-up creates.
///
/// # Errors
///
/// Database errors only.
pub fn set_account_kinds(conn: &Connection, kinds: AccountKinds) -> Result<(), DbError> {
    let value = match kinds {
        AccountKinds::Standard => "standard",
        AccountKinds::E2ee => "e2ee",
        AccountKinds::Choice => "choice",
    };
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('account_kinds', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [value],
    )?;
    Ok(())
}

/// The OPAQUE record of an e2ee account, with how it stretches the
/// password (`app_e2ee::Ksf`'s number).
///
/// # Errors
///
/// Database errors only.
pub fn record(conn: &Connection, user_id: &str) -> Result<Option<(Vec<u8>, u8)>, DbError> {
    let found: Option<(Option<Vec<u8>>, u8)> = conn
        .query_row(
            "SELECT opaque_record, opaque_ksf FROM users WHERE id = ?1 AND kind = 'e2ee'",
            [user_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(found.and_then(|(record, ksf)| record.map(|r| (r, ksf))))
}

/// Whether any e2ee account still has a record from before stronger
/// stretching. While one does, an unknown username answers either number,
/// so the number does not tell an old account from no account.
///
/// # Errors
///
/// Database errors only.
pub fn old_records_remain(conn: &Connection) -> Result<bool, DbError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM users WHERE kind = 'e2ee' AND opaque_ksf < ?1)",
        [app_protocol::e2ee::KSF_CURRENT],
        |r| r.get(0),
    )?)
}

/// The wrapped keys of an e2ee account.
///
/// # Errors
///
/// Database errors, or a stored bundle that no longer reads.
pub fn keys(conn: &Connection, user_id: &str) -> Result<Option<KeyBundle>, DbError> {
    let text: Option<String> = conn
        .query_row(
            "SELECT key_bundle FROM users WHERE id = ?1 AND kind = 'e2ee'",
            [user_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    text.map(|t| serde_json::from_str(&t).map_err(|e| DbError::Data(e.to_string())))
        .transpose()
}

/// The stored form of a recovery verifier.
pub fn verifier_hash(verifier: &str) -> [u8; 32] {
    Sha256::digest(verifier.trim().as_bytes()).into()
}

/// Whether `verifier` proves the account's recovery code.
///
/// # Errors
///
/// Database errors only.
pub fn verifier_matches(conn: &Connection, user_id: &str, verifier: &str) -> Result<bool, DbError> {
    let stored: Option<Vec<u8>> = conn
        .query_row(
            "SELECT recovery_verifier FROM users WHERE id = ?1 AND kind = 'e2ee'",
            [user_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    // both sides are sha-256 outputs of the same length, the comparison
    // reveals nothing an attacker can steer
    Ok(stored.is_some_and(|s| s.as_slice() == verifier_hash(verifier)))
}

/// Makes a freshly inserted account an e2ee one.
///
/// # Errors
///
/// Database errors, or a bundle that does not serialise.
pub fn make_e2ee(
    conn: &Connection,
    user_id: &str,
    record: &[u8],
    ksf: u8,
    keys: &KeyBundle,
    verifier: &str,
) -> Result<(), DbError> {
    let bundle = serde_json::to_string(keys).map_err(|e| DbError::Data(e.to_string()))?;
    conn.execute(
        "UPDATE users SET kind = 'e2ee', password_hash = '', opaque_record = ?2, key_bundle = ?3,
         recovery_verifier = ?4, opaque_ksf = ?5 WHERE id = ?1",
        params![
            user_id,
            record,
            bundle,
            verifier_hash(verifier).as_slice(),
            ksf
        ],
    )?;
    Ok(())
}

/// A new password, or the same one stretched harder: the new record and
/// the keys with their new wrapping.
///
/// # Errors
///
/// Database errors, or a bundle that does not serialise.
pub fn set_password(
    conn: &Connection,
    user_id: &str,
    record: &[u8],
    ksf: u8,
    keys: &KeyBundle,
) -> Result<(), DbError> {
    let bundle = serde_json::to_string(keys).map_err(|e| DbError::Data(e.to_string()))?;
    conn.execute(
        "UPDATE users SET opaque_record = ?2, key_bundle = ?3, opaque_ksf = ?4
         WHERE id = ?1 AND kind = 'e2ee'",
        params![user_id, record, bundle, ksf],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    /// The OPAQUE setup is not in the database
    /// as it is, and one stored plain by 0.2 is sealed when read.
    #[tokio::test]
    async fn the_opaque_setup_is_sealed_by_the_master_key() {
        let db = Db::in_memory().unwrap();
        let master = std::sync::Arc::new(MasterKey::generate());
        let m = master.clone();
        let (plain, raw, same) = db
            .call(move |c| {
                let old = Setup::generate();
                let plain = STANDARD.encode(old.to_bytes().as_slice());
                c.execute(
                    "INSERT INTO instance (key, value) VALUES ('opaque_setup', ?1)",
                    [&plain],
                )?;
                let read = setup(c, &m)?;
                let raw: String = c.query_row(
                    "SELECT value FROM instance WHERE key = 'opaque_setup'",
                    [],
                    |r| r.get(0),
                )?;
                let again = setup(c, &m)?;
                Ok((
                    plain,
                    raw,
                    read.to_bytes() == old.to_bytes() && again.to_bytes() == old.to_bytes(),
                ))
            })
            .await
            .unwrap();
        assert!(same, "the same setup, before and after sealing");
        assert!(raw.starts_with(SEALED) && !raw.contains(&plain), "{raw}");
        let other = MasterKey::generate();
        assert!(db.call(move |c| setup(c, &other)).await.is_err());
    }
}
