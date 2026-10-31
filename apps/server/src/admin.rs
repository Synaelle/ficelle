// SPDX-License-Identifier: AGPL-3.0-or-later

//! Storage for the admin api: the instance at a glance, people as an admin
//! sees them, groups and the security log. Plain functions over a
//! connection, like [`crate::accounts::store`].

use app_protocol::account::Role;
use app_protocol::admin::{AdminUser, AuditEntry, Group, GroupMember, Overview};
use rusqlite::{Connection, OptionalExtension, params};

use crate::accounts::{e2ee, store};
use crate::db::DbError;

/// Whether the server is for its admin alone ("Sync for myself" at setup).
///
/// # Errors
///
/// Database errors only.
pub fn personal(conn: &Connection) -> Result<bool, DbError> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'server_use'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.as_deref() == Some("personal"))
}

/// Sets whether the server is for its admin alone.
///
/// # Errors
///
/// Database errors only.
pub fn set_personal(conn: &Connection, personal: bool) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('server_use', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [if personal { "personal" } else { "instance" }],
    )?;
    Ok(())
}

/// Counts for the overview.
///
/// # Errors
///
/// Database errors only.
pub fn overview(conn: &Connection) -> Result<Overview, DbError> {
    let count = |sql: &str| -> Result<u64, DbError> {
        let n: i64 = conn.query_row(sql, [], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    };
    let created_at: String = conn.query_row(
        "SELECT value FROM instance WHERE key = 'created_at'",
        [],
        |r| r.get(0),
    )?;
    Ok(Overview {
        version: app_brand::VERSION.into(),
        created_at,
        users: count("SELECT COUNT(*) FROM users")?,
        admins: count("SELECT COUNT(*) FROM users WHERE role = 'admin'")?,
        disabled: count("SELECT COUNT(*) FROM users WHERE disabled_at IS NOT NULL")?,
        groups: count("SELECT COUNT(*) FROM groups")?,
        devices: count("SELECT COUNT(*) FROM devices WHERE revoked_at IS NULL")?,
        registration: store::registration(conn)?,
        account_kinds: e2ee::account_kinds(conn)?,
        e2ee: count("SELECT COUNT(*) FROM users WHERE kind = 'e2ee'")?,
        coediting: crate::sync::coediting(conn)?,
        admins_need_second_step: crate::accounts::factors::admins_need_second_step(conn)?,
        personal: personal(conn)?,
    })
}

const ADMIN_USER_SELECT: &str = "SELECT u.id, u.username, u.display_name, u.role, u.created_at,
        u.disabled_at, u.locked_until,
        (SELECT MAX(last_seen_at) FROM devices d WHERE d.user_id = u.id),
        (SELECT COUNT(*) FROM devices d WHERE d.user_id = u.id AND d.revoked_at IS NULL),
        (SELECT group_concat(g.name, char(31)) FROM group_members m JOIN groups g ON g.id = m.group_id
         WHERE m.user_id = u.id),
        u.totp_secret IS NOT NULL OR EXISTS (SELECT 1 FROM passkeys p WHERE p.user_id = u.id),
        u.kind,
        CASE WHEN u.email_verified_at IS NULL THEN NULL ELSE u.email END,
        u.delete_at
    FROM users u";

fn admin_user_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdminUser> {
    let groups: Option<String> = row.get(9)?;
    let mut groups: Vec<String> = groups
        .map(|g| g.split('\u{1f}').map(str::to_owned).collect())
        .unwrap_or_default();
    groups.sort_by_key(|g| g.to_lowercase());
    Ok(AdminUser {
        id: row.get(0)?,
        username: row.get(1)?,
        display_name: row.get(2)?,
        role: if row.get::<_, String>(3)? == "admin" {
            Role::Admin
        } else {
            Role::User
        },
        created_at: row.get(4)?,
        disabled_at: row.get(5)?,
        locked_until: row.get(6)?,
        last_seen_at: row.get(7)?,
        devices: u64::try_from(row.get::<_, i64>(8)?).unwrap_or(0),
        groups,
        second_factor: row.get(10)?,
        kind: e2ee::kind_of(&row.get::<_, String>(11)?),
        email: row.get(12)?,
        delete_at: row.get(13)?,
    })
}

/// A page of accounts by username, filtered by part of the username or
/// shown name.
///
/// # Errors
///
/// Database errors only.
pub fn list_users(
    conn: &Connection,
    query: &str,
    offset: u32,
    limit: u32,
) -> Result<(u64, Vec<AdminUser>), DbError> {
    let pattern = format!("%{}%", query.replace(['%', '_'], ""));
    let filter = "WHERE u.username LIKE ?1 OR u.display_name LIKE ?1";
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM users u {filter}"),
        [&pattern],
        |r| r.get(0),
    )?;
    let sql = format!("{ADMIN_USER_SELECT} {filter} ORDER BY u.username LIMIT ?2 OFFSET ?3");
    let mut stmt = conn.prepare(&sql)?;
    let items = stmt
        .query_map(params![pattern, limit, offset], admin_user_from)?
        .collect::<rusqlite::Result<_>>()?;
    Ok((u64::try_from(total).unwrap_or(0), items))
}

/// One account.
///
/// # Errors
///
/// Database errors only.
pub fn admin_user(conn: &Connection, id: &str) -> Result<Option<AdminUser>, DbError> {
    let sql = format!("{ADMIN_USER_SELECT} WHERE u.id = ?1");
    Ok(conn.query_row(&sql, [id], admin_user_from).optional()?)
}

/// Admins that are turned on, leaving out `except`. Zero means changing
/// `except` would leave nobody to run the server.
///
/// # Errors
///
/// Database errors only.
pub fn other_active_admins(conn: &Connection, except: &str) -> Result<i64, DbError> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM users WHERE role = 'admin' AND disabled_at IS NULL AND id != ?1",
        [except],
        |r| r.get(0),
    )?)
}

/// Changes the fields given. Turning an account off signs out its devices.
///
/// # Errors
///
/// Database errors only.
pub fn update_user(
    conn: &Connection,
    id: &str,
    display_name: Option<&str>,
    role: Option<Role>,
    disabled: Option<bool>,
    unlock: bool,
    now: i64,
) -> Result<(), DbError> {
    if let Some(name) = display_name {
        conn.execute(
            "UPDATE users SET display_name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
    }
    if let Some(role) = role {
        let text = if role == Role::Admin { "admin" } else { "user" };
        conn.execute(
            "UPDATE users SET role = ?2 WHERE id = ?1",
            params![id, text],
        )?;
        crate::accounts::factors::guard_admin(conn, id)?;
    }
    match disabled {
        Some(true) => {
            conn.execute(
                "UPDATE users SET disabled_at = ?2 WHERE id = ?1 AND disabled_at IS NULL",
                params![id, now],
            )?;
            store::revoke_other_devices(conn, id, None, now)?;
        }
        Some(false) => {
            conn.execute("UPDATE users SET disabled_at = NULL WHERE id = ?1", [id])?;
        }
        None => {}
    }
    if unlock {
        store::record_success(conn, id)?;
    }
    Ok(())
}

/// Deletes an account with its devices and group memberships. Invites it
/// made stay, without a creator. False when there was no such account.
///
/// # Errors
///
/// Database errors only.
pub fn delete_user(conn: &Connection, id: &str) -> Result<bool, DbError> {
    Ok(conn.execute("DELETE FROM users WHERE id = ?1", [id])? == 1)
}

/// A group name as stored, `None` when unusable: 1 to 64 characters without
/// control characters, trimmed.
pub fn group_name(raw: &str) -> Option<String> {
    let clean: String = raw.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    (!clean.is_empty() && clean.chars().count() <= 64).then(|| clean.to_owned())
}

fn group_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<Group> {
    Ok(Group {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        members: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
    })
}

const GROUP_SELECT: &str = "SELECT g.id, g.name, g.created_at,
    (SELECT COUNT(*) FROM group_members m WHERE m.group_id = g.id) FROM groups g";

/// Every group, by name.
///
/// # Errors
///
/// Database errors only.
pub fn list_groups(conn: &Connection) -> Result<Vec<Group>, DbError> {
    let mut stmt = conn.prepare(&format!("{GROUP_SELECT} ORDER BY g.name COLLATE NOCASE"))?;
    Ok(stmt
        .query_map([], group_from)?
        .collect::<rusqlite::Result<_>>()?)
}

/// The groups an account belongs to, by name.
///
/// # Errors
///
/// Database errors only.
pub fn groups_of(conn: &Connection, user_id: &str) -> Result<Vec<Group>, DbError> {
    let sql = format!(
        "{GROUP_SELECT} WHERE g.id IN (SELECT group_id FROM group_members WHERE user_id = ?1)
         ORDER BY g.name COLLATE NOCASE"
    );
    let mut stmt = conn.prepare(&sql)?;
    Ok(stmt
        .query_map([user_id], group_from)?
        .collect::<rusqlite::Result<_>>()?)
}

/// One group.
///
/// # Errors
///
/// Database errors only.
pub fn group(conn: &Connection, id: &str) -> Result<Option<Group>, DbError> {
    Ok(conn
        .query_row(&format!("{GROUP_SELECT} WHERE g.id = ?1"), [id], group_from)
        .optional()?)
}

/// Whether another group already has this name.
///
/// # Errors
///
/// Database errors only.
pub fn group_name_taken(
    conn: &Connection,
    name: &str,
    except: Option<&str>,
) -> Result<bool, DbError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM groups WHERE name = ?1 AND (?2 IS NULL OR id != ?2))",
        params![name, except],
        |r| r.get(0),
    )?)
}

/// Creates a group. `None` when the name is taken.
///
/// # Errors
///
/// Database errors only.
pub fn create_group(conn: &Connection, name: &str, now: i64) -> Result<Option<Group>, DbError> {
    if group_name_taken(conn, name, None)? {
        return Ok(None);
    }
    let id = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO groups (id, name, created_at) VALUES (?1, ?2, ?3)",
        params![id, name, now],
    )?;
    group(conn, &id)
}

/// Renames a group. False when there is no such group.
///
/// # Errors
///
/// Database errors only.
pub fn rename_group(conn: &Connection, id: &str, name: &str) -> Result<bool, DbError> {
    Ok(conn.execute(
        "UPDATE groups SET name = ?2 WHERE id = ?1",
        params![id, name],
    )? == 1)
}

/// Deletes a group, not its members. False when there was no such group.
///
/// # Errors
///
/// Database errors only.
pub fn delete_group(conn: &Connection, id: &str) -> Result<bool, DbError> {
    Ok(conn.execute("DELETE FROM groups WHERE id = ?1", [id])? == 1)
}

/// Members of a group, by username.
///
/// # Errors
///
/// Database errors only.
pub fn members(conn: &Connection, group_id: &str) -> Result<Vec<GroupMember>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT u.id, u.username, u.display_name, m.added_at FROM group_members m
         JOIN users u ON u.id = m.user_id WHERE m.group_id = ?1 ORDER BY u.username",
    )?;
    let rows = stmt.query_map([group_id], |r| {
        Ok(GroupMember {
            user_id: r.get(0)?,
            username: r.get(1)?,
            display_name: r.get(2)?,
            added_at: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Adds a member, nothing happens if already there.
///
/// # Errors
///
/// Database errors only.
pub fn add_member(
    conn: &Connection,
    group_id: &str,
    user_id: &str,
    now: i64,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT OR IGNORE INTO group_members (group_id, user_id, added_at) VALUES (?1, ?2, ?3)",
        params![group_id, user_id, now],
    )?;
    Ok(())
}

/// Removes a member. False when they were not one.
///
/// # Errors
///
/// Database errors only.
pub fn remove_member(conn: &Connection, group_id: &str, user_id: &str) -> Result<bool, DbError> {
    Ok(conn.execute(
        "DELETE FROM group_members WHERE group_id = ?1 AND user_id = ?2",
        params![group_id, user_id],
    )? == 1)
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
    use super::*;
    use crate::db::Db;

    /// An admin with no second factor, made
    /// so or left so, does not act as admin until they add one.
    #[tokio::test]
    async fn admins_need_a_second_factor_whatever_the_way() {
        use crate::accounts::factors;
        let db = Db::in_memory().unwrap();
        db.call(|c| {
            let anna = store::insert_user(c, "anna", "Anna", "h", Role::User, 1)?.unwrap();
            update_user(c, &anna.id, None, Some(Role::Admin), None, false, 2)?;
            let user = |c: &Connection| Ok::<_, DbError>(store::user_by_id(c, &anna.id)?.unwrap());
            assert!(!user(c)?.acts_as_admin(), "promoted without a factor");

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
            assert!(!user(c)?.acts_as_admin(), "factors reset by another admin");
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn groups_members_and_the_user_view() {
        let db = Db::in_memory().unwrap();
        db.call(|c| {
            let anna = store::insert_user(c, "anna", "Anna", "h", Role::User, 1)?.unwrap();
            let team = create_group(c, "Team", 2)?.unwrap();
            assert!(
                create_group(c, "team", 3)?.is_none(),
                "names are unique whatever the case"
            );
            create_group(c, "Book club", 4)?.unwrap();
            add_member(c, &team.id, &anna.id, 5)?;
            add_member(c, &team.id, &anna.id, 6)?;
            assert_eq!(members(c, &team.id)?.len(), 1, "added once");

            let seen = admin_user(c, &anna.id)?.unwrap();
            assert_eq!(seen.groups, vec!["Team".to_string()]);
            assert_eq!(groups_of(c, &anna.id)?.len(), 1);
            let (total, page) = list_users(c, "ANN", 0, 10)?;
            assert_eq!((total, page.len()), (1, 1));

            assert!(delete_user(c, &anna.id)?);
            assert_eq!(
                group(c, &team.id)?.unwrap().members,
                0,
                "memberships go with the account"
            );
            Ok(())
        })
        .await
        .unwrap();
    }

    #[test]
    fn group_names() {
        assert_eq!(group_name("  Team\u{7} "), Some("Team".into()));
        assert_eq!(group_name("   "), None);
        assert_eq!(group_name(&"x".repeat(65)), None);
    }
}
