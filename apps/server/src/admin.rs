// SPDX-License-Identifier: AGPL-3.0-or-later

//! Storage for the admin api: the instance at a glance and the security
//! log. Plain functions over a connection, like [`crate::accounts::store`].

use app_protocol::admin::{AuditEntry, Overview};
use rusqlite::{Connection, params};

use crate::db::DbError;

/// Counts for the overview.
///
/// # Errors
///
/// Database errors only.
pub fn overview(conn: &Connection) -> Result<Overview, DbError> {
    let devices: i64 = conn.query_row(
        "SELECT COUNT(*) FROM devices WHERE revoked_at IS NULL",
        [],
        |r| r.get(0),
    )?;
    let created_at: String = conn.query_row(
        "SELECT value FROM instance WHERE key = 'created_at'",
        [],
        |r| r.get(0),
    )?;
    Ok(Overview {
        version: app_brand::VERSION.into(),
        created_at,
        devices: u64::try_from(devices).unwrap_or(0),
        coediting: crate::sync::coediting(conn)?,
        admins_need_second_step: crate::accounts::factors::admins_need_second_step(conn)?,
    })
}

/// Security log entries older than `before`, newest first, optionally for
/// one event name or one account.
///
/// # Errors
///
/// Database errors only.
pub fn audit_page(
    conn: &Connection,
    event: Option<&str>,
    user_id: Option<&str>,
    before: Option<i64>,
    limit: u32,
) -> Result<Vec<AuditEntry>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.at, a.event, a.user_id, u.username, a.device_id, a.ip, a.detail
         FROM audit_log a LEFT JOIN users u ON u.id = a.user_id
         WHERE (?1 IS NULL OR a.event = ?1) AND (?2 IS NULL OR a.user_id = ?2) AND (?3 IS NULL OR a.id < ?3)
         ORDER BY a.id DESC LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![event, user_id, before, limit], |r| {
        Ok(AuditEntry {
            id: r.get(0)?,
            at: r.get(1)?,
            event: r.get(2)?,
            user_id: r.get(3)?,
            username: r.get(4)?,
            device_id: r.get(5)?,
            ip: r.get(6)?,
            detail: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use app_protocol::account::Role;

    use crate::accounts::{factors, store};
    use crate::db::{Db, DbError};
    use rusqlite::Connection;

    /// An admin with no second factor, made so or left so, does not act as
    /// admin until they add one.
    #[tokio::test]
    async fn admins_need_a_second_factor_whatever_the_way() {
        let db = Db::in_memory().unwrap();
        db.call(|c| {
            let anna = store::insert_user(c, "anna", "Anna", "h", Role::Admin, 1)?.unwrap();
            factors::guard_admin(c, &anna.id)?;
            let user = |c: &Connection| Ok::<_, DbError>(store::user_by_id(c, &anna.id)?.unwrap());
            assert!(!user(c)?.acts_as_admin(), "made without a factor");

            factors::set_totp_pending(
                c,
                &crate::keys::MasterKey::generate(),
                &anna.id,
                "JBSWY3DPEHPK3PXP",
            )?;
            factors::activate_totp(c, &anna.id, 1)?;
            store::set_factor_required(c, &anna.id, false)?;
            assert!(user(c)?.acts_as_admin());
            factors::disable_totp(c, &anna.id)?;
            assert!(!user(c)?.acts_as_admin(), "their last factor removed");

            factors::set_totp_pending(
                c,
                &crate::keys::MasterKey::generate(),
                &anna.id,
                "JBSWY3DPEHPK3PXP",
            )?;
            factors::activate_totp(c, &anna.id, 2)?;
            store::set_factor_required(c, &anna.id, false)?;
            factors::reset_second_factors(c, &anna.id)?;
            assert!(
                !user(c)?.acts_as_admin(),
                "factors reset from the command line"
            );
            Ok(())
        })
        .await
        .unwrap();
    }
}
