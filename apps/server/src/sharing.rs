// SPDX-License-Identifier: AGPL-3.0-or-later

//! Storage for sharing: shared spaces, who is in them, the keys an owner's
//! devices sealed for the members of an e2ee one, and the key pairs those
//! keys are sealed to. A shared space keeps its changes like an account's
//! own space, see [`crate::sync`].
//!
//! Who is in a space is worked out when asked: the owner, the people added
//! one by one, and everyone in the groups added, less those who left it
//! themselves. An e2ee space only counts e2ee accounts, a standard member
//! of a group added to it is left out. When someone with the key of an
//! e2ee space's latest epoch is no longer in it, a new epoch starts: the
//! owner's devices make a new key and seal it for those still in, the
//! one who left never gets it.

use app_protocol::e2ee::AccountKind;
use app_protocol::sync::{
    AccountKeyPair, GrantWanted, MemberChoice, Person, SharedSpace, SpaceGrant, SpaceMember,
    SpaceRole,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db::DbError;
use crate::keys::MasterKey;

fn role_text(role: SpaceRole) -> &'static str {
    match role {
        SpaceRole::Owner => "owner",
        SpaceRole::Edit => "edit",
        SpaceRole::Read => "read",
    }
}

fn role_of_text(text: &str) -> SpaceRole {
    match text {
        "edit" => SpaceRole::Edit,
        "owner" => SpaceRole::Owner,
        _ => SpaceRole::Read,
    }
}

fn kind_text(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::Standard => "standard",
        AccountKind::E2ee => "e2ee",
    }
}

/// A shared space's own row.
#[derive(Debug, Clone)]
pub struct Shared {
    /// Its id.
    pub id: String,
    /// The account that shared it.
    pub owner_id: String,
    /// The owner's kind when it was shared.
    pub kind: AccountKind,
    /// E2ee: the latest key epoch.
    pub epoch: u32,
}

/// The shared space with this id, if there is one.
///
/// # Errors
///
/// Database errors only.
pub fn shared(conn: &Connection, id: &str) -> Result<Option<Shared>, DbError> {
    Ok(conn
        .query_row(
            "SELECT owner_id, kind, epoch FROM sync_spaces WHERE id = ?1 AND shared = 1",
            [id],
            |row| {
                Ok(Shared {
                    id: id.to_owned(),
                    owner_id: row.get(0)?,
                    kind: crate::accounts::e2ee::kind_of(&row.get::<_, String>(1)?),
                    epoch: row.get(2)?,
                })
            },
        )
        .optional()?)
}

/// Everyone in a shared space and their role, the owner first.
///
/// # Errors
///
/// Database errors only.
pub fn people(conn: &Connection, space: &Shared) -> Result<Vec<(String, SpaceRole)>, DbError> {
    let mut stmt = conn.prepare_cached(
        "WITH found (user_id, role) AS (
             SELECT user_id, role FROM sync_members WHERE space_id = ?1 AND user_id IS NOT NULL
             UNION ALL
             SELECT g.user_id, m.role FROM sync_members m
             JOIN group_members g ON g.group_id = m.group_id WHERE m.space_id = ?1
         )
         SELECT f.user_id, max(f.role = 'edit') FROM found f JOIN users u ON u.id = f.user_id
         WHERE f.user_id != ?2 AND (?3 = 'standard' OR u.kind = 'e2ee')
           AND NOT EXISTS (SELECT 1 FROM sync_left l WHERE l.space_id = ?1 AND l.user_id = f.user_id)
         GROUP BY f.user_id ORDER BY f.user_id",
    )?;
    let mut out = vec![(space.owner_id.clone(), SpaceRole::Owner)];
    let rows = stmt.query_map(
        params![space.id, space.owner_id, kind_text(space.kind)],
        |row| {
            let edit: bool = row.get(1)?;
            Ok((
                row.get::<_, String>(0)?,
                if edit {
                    SpaceRole::Edit
                } else {
                    SpaceRole::Read
                },
            ))
        },
    )?;
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// An account's role in a shared space, none when it is not in it.
///
/// # Errors
///
/// Database errors only.
pub fn role_in(
    conn: &Connection,
    space: &Shared,
    user_id: &str,
) -> Result<Option<SpaceRole>, DbError> {
    Ok(people(conn, space)?
        .into_iter()
        .find(|(id, _)| id == user_id)
        .map(|(_, role)| role))
}

/// Ids of the shared spaces an account may be in: owned, or with the
/// account or one of its groups added.
fn candidates(conn: &Connection, user_id: &str) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM sync_spaces WHERE owner_id = ?1 AND shared = 1
         UNION SELECT space_id FROM sync_members WHERE user_id = ?1
         UNION SELECT m.space_id FROM sync_members m
               JOIN group_members g ON g.group_id = m.group_id WHERE g.user_id = ?1",
    )?;
    let ids = stmt
        .query_map([user_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

/// The shared spaces an account is in, as its devices see them: its role,
/// its keys for an e2ee one and, for the owner, the keys still to seal.
///
/// # Errors
///
/// Database errors only.
pub fn spaces_of(conn: &Connection, user_id: &str) -> Result<Vec<SharedSpace>, DbError> {
    let mut out = Vec::new();
    for id in candidates(conn, user_id)? {
        let Some(space) = shared(conn, &id)? else {
            continue;
        };
        let people = people(conn, &space)?;
        let Some(role) = people
            .iter()
            .find(|(id, _)| id == user_id)
            .map(|(_, role)| *role)
        else {
            continue;
        };
        let owner: String = conn.query_row(
            "SELECT username FROM users WHERE id = ?1",
            [&space.owner_id],
            |row| row.get(0),
        )?;
        let owner_key = if space.kind == AccountKind::E2ee {
            public_key(conn, &space.owner_id)?.unwrap_or_default()
        } else {
            String::new()
        };
        let (grants, grants_wanted) = if space.kind == AccountKind::E2ee {
            let grants = grants_of(conn, &space.id, user_id)?;
            let wanted = if role == SpaceRole::Owner {
                wanted(conn, &space, &people)?
            } else {
                Vec::new()
            };
            (grants, wanted)
        } else {
            (Vec::new(), Vec::new())
        };
        out.push(SharedSpace {
            head: crate::sync::head(conn, &space.id)?,
            id: space.id,
            role,
            kind: space.kind,
            owner,
            owner_key,
            epoch: space.epoch,
            grants,
            grants_wanted,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn grants_of(conn: &Connection, space: &str, user_id: &str) -> Result<Vec<SpaceGrant>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT epoch, grant_data FROM sync_grants WHERE space_id = ?1 AND user_id = ?2 ORDER BY epoch",
    )?;
    let rows = stmt
        .query_map(params![space, user_id], |row| {
            Ok(SpaceGrant {
                epoch: row.get(0)?,
                grant: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Members with a public key and no key for some epoch. A member added
/// after a removal needs the older epochs too, to read what came before.
fn wanted(
    conn: &Connection,
    space: &Shared,
    people: &[(String, SpaceRole)],
) -> Result<Vec<GrantWanted>, DbError> {
    let mut out = Vec::new();
    for (user_id, _) in people {
        let Some(public_key) = public_key(conn, user_id)? else {
            continue;
        };
        // for the owner to recognise who waits, their devices let in only
        // people they chose
        let name: String = conn
            .query_row(
                "SELECT username FROM users WHERE id = ?1",
                [user_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_default();
        let have: Vec<u32> = grants_of(conn, &space.id, user_id)?
            .into_iter()
            .map(|g| g.epoch)
            .collect();
        for epoch in 1..=space.epoch {
            if !have.contains(&epoch) {
                out.push(GrantWanted {
                    user_id: user_id.clone(),
                    public_key: public_key.clone(),
                    epoch,
                    name: name.clone(),
                });
            }
        }
    }
    Ok(out)
}

/// What is wrong with a choice of members, in words for the owner.
pub type Refusal = &'static str;

/// Makes a shared space for `owner` with these members. Returns its id and
/// the accounts in it.
///
/// # Errors
///
/// Database errors. The inner error names a member choice that cannot be.
pub fn create(
    conn: &Connection,
    master: &MasterKey,
    owner_id: &str,
    kind: AccountKind,
    members: &[MemberChoice],
    now: i64,
) -> Result<Result<(String, Vec<String>), Refusal>, DbError> {
    let id = uuid::Uuid::now_v7().to_string();
    let data_key = match kind {
        AccountKind::Standard => Some(master.new_space_key(&id).1),
        AccountKind::E2ee => None,
    };
    conn.execute(
        "INSERT INTO sync_spaces (id, owner_id, data_key, created_at, shared, kind, epoch)
         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)",
        params![
            id,
            owner_id,
            data_key,
            now,
            kind_text(kind),
            u32::from(kind == AccountKind::E2ee)
        ],
    )?;
    let space = Shared {
        id: id.clone(),
        owner_id: owner_id.to_owned(),
        kind,
        epoch: u32::from(kind == AccountKind::E2ee),
    };
    Ok(set_members(conn, &space, members, now)?.map(|touched| (id, touched)))
}

/// Puts these members in place of the space's. Returns the accounts in it
/// before or after, who should hear of it.
///
/// # Errors
///
/// Database errors. The inner error names a member choice that cannot be.
pub fn set_members(
    conn: &Connection,
    space: &Shared,
    members: &[MemberChoice],
    now: i64,
) -> Result<Result<Vec<String>, Refusal>, DbError> {
    if members.len() > 500 {
        return Ok(Err("share with at most 500 people and groups"));
    }
    for choice in members {
        if let Err(why) = check_choice(conn, space, choice)? {
            return Ok(Err(why));
        }
    }
    let before = people(conn, space)?;
    conn.execute("DELETE FROM sync_members WHERE space_id = ?1", [&space.id])?;
    for choice in members {
        conn.execute(
            "INSERT OR REPLACE INTO sync_members (space_id, user_id, group_id, role, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                space.id,
                choice.user_id,
                choice.group_id,
                role_text(choice.role),
                now
            ],
        )?;
        // added again by name: whatever they left, they are back
        if let Some(user) = &choice.user_id {
            conn.execute(
                "DELETE FROM sync_left WHERE space_id = ?1 AND user_id = ?2",
                params![space.id, user],
            )?;
        }
    }
    recheck(conn, space)?;
    let mut touched: Vec<String> = before.into_iter().map(|(id, _)| id).collect();
    touched.extend(people(conn, space)?.into_iter().map(|(id, _)| id));
    touched.sort();
    touched.dedup();
    Ok(Ok(touched))
}

fn check_choice(
    conn: &Connection,
    space: &Shared,
    choice: &MemberChoice,
) -> Result<Result<(), Refusal>, DbError> {
    if choice.role == SpaceRole::Owner {
        return Ok(Err(
            "a member reads or edits, the owner stays who shared it",
        ));
    }
    match (&choice.user_id, &choice.group_id) {
        (Some(user), None) => {
            if *user == space.owner_id {
                return Ok(Err("the owner is in it already"));
            }
            let kind: Option<String> = conn
                .query_row("SELECT kind FROM users WHERE id = ?1", [user], |row| {
                    row.get(0)
                })
                .optional()?;
            match kind.as_deref() {
                None => Ok(Err("no such account")),
                Some("e2ee") => Ok(Ok(())),
                Some(_) if space.kind == AccountKind::E2ee => Ok(Err(
                    "end-to-end encrypted notes are shared with end-to-end encrypted accounts only",
                )),
                Some(_) => Ok(Ok(())),
            }
        }
        (None, Some(group)) => {
            let found: bool = conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM groups WHERE id = ?1)",
                [group],
                |row| row.get(0),
            )?;
            if !found {
                return Ok(Err("no such group"));
            }
            // a group gets notes only from its own members or an admin, not
            // from anyone who knows its id. One it has already stays, when
            // its owner has left the group since.
            let allowed: bool = conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM group_members WHERE group_id = ?1 AND user_id = ?2)
                     OR EXISTS (SELECT 1 FROM users WHERE id = ?2 AND role = 'admin')
                     OR EXISTS (SELECT 1 FROM sync_members WHERE space_id = ?3 AND group_id = ?1)",
                params![group, space.owner_id, space.id],
                |row| row.get(0),
            )?;
            Ok(if allowed {
                Ok(())
            } else {
                Err("share with a group you are in")
            })
        }
        _ => Ok(Err("a member is an account or a group")),
    }
}

/// Starts a new epoch when someone holding the latest key is no longer in
/// the space. True when it did.
///
/// # Errors
///
/// Database errors only.
pub fn recheck(conn: &Connection, space: &Shared) -> Result<bool, DbError> {
    if space.kind != AccountKind::E2ee {
        return Ok(false);
    }
    let people: Vec<String> = people(conn, space)?.into_iter().map(|(id, _)| id).collect();
    let mut stmt =
        conn.prepare_cached("SELECT user_id FROM sync_grants WHERE space_id = ?1 AND epoch = ?2")?;
    let holders = stmt
        .query_map(params![space.id, space.epoch], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if holders.iter().all(|h| people.contains(h)) {
        return Ok(false);
    }
    conn.execute(
        "UPDATE sync_spaces SET epoch = epoch + 1 WHERE id = ?1",
        [&space.id],
    )?;
    Ok(true)
}

/// Runs a change to a group (a member added or removed, the group gone)
/// and starts a new epoch in the e2ee spaces it is in where someone lost
/// the key. Returns what the change returned and the accounts of those
/// spaces before or after, who should hear of it.
///
/// # Errors
///
/// Database errors, and those of the change.
pub fn around_group<T>(
    conn: &Connection,
    group_id: &str,
    change: impl FnOnce(&Connection) -> Result<T, DbError>,
) -> Result<(T, Vec<String>), DbError> {
    let mut touched = Vec::new();
    let mut spaces = Vec::new();
    for id in spaces_with_group(conn, group_id)? {
        if let Some(space) = shared(conn, &id)? {
            touched.extend(people(conn, &space)?.into_iter().map(|(id, _)| id));
            spaces.push(space);
        }
    }
    let out = change(conn)?;
    for space in &spaces {
        recheck(conn, space)?;
        touched.extend(people(conn, space)?.into_iter().map(|(id, _)| id));
    }
    touched.sort();
    touched.dedup();
    Ok((out, touched))
}

/// Shared spaces a group is in.
fn spaces_with_group(conn: &Connection, group_id: &str) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare("SELECT space_id FROM sync_members WHERE group_id = ?1")?;
    let ids = stmt
        .query_map([group_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

/// A member leaves a space on their own. Returns who should hear of it.
///
/// # Errors
///
/// Database errors only.
pub fn leave(conn: &Connection, space: &Shared, user_id: &str) -> Result<Vec<String>, DbError> {
    let touched: Vec<String> = people(conn, space)?.into_iter().map(|(id, _)| id).collect();
    conn.execute(
        "DELETE FROM sync_members WHERE space_id = ?1 AND user_id = ?2",
        params![space.id, user_id],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO sync_left (space_id, user_id) VALUES (?1, ?2)",
        params![space.id, user_id],
    )?;
    recheck(conn, space)?;
    Ok(touched)
}

/// Stops sharing: the space and all its changes go. Returns who was in it.
///
/// # Errors
///
/// Database errors only.
pub fn delete(conn: &Connection, space: &Shared) -> Result<Vec<String>, DbError> {
    let touched: Vec<String> = people(conn, space)?.into_iter().map(|(id, _)| id).collect();
    conn.execute("DELETE FROM sync_spaces WHERE id = ?1", [&space.id])?;
    Ok(touched)
}

/// Keeps keys an owner's device sealed for members. The first key for a
/// member and epoch stays: two devices of the owner making keys at once
/// end with the one the server took first, which they both open. Returns
/// the members who got one.
///
/// # Errors
///
/// Database errors. The inner error names a grant that cannot be.
pub fn give(
    conn: &Connection,
    space: &Shared,
    epoch: u32,
    grants: &[(String, String)],
) -> Result<Result<Vec<String>, Refusal>, DbError> {
    if epoch == 0 || epoch > space.epoch {
        return Ok(Err("no such key epoch"));
    }
    let people: Vec<String> = people(conn, space)?.into_iter().map(|(id, _)| id).collect();
    if grants.iter().any(|(user, _)| !people.contains(user)) {
        return Ok(Err("a key for someone not in the space"));
    }
    let mut got = Vec::new();
    for (user, grant) in grants {
        let new = conn.execute(
            "INSERT OR IGNORE INTO sync_grants (space_id, user_id, epoch, grant_data) VALUES (?1, ?2, ?3, ?4)",
            params![space.id, user, epoch, grant],
        )?;
        if new == 1 {
            got.push(user.clone());
        }
    }
    Ok(Ok(got))
}

/// Who a space is shared with, as the owner chose: the owner, then people
/// and groups by name.
///
/// # Errors
///
/// Database errors only.
pub fn members(conn: &Connection, space: &Shared) -> Result<Vec<SpaceMember>, DbError> {
    let person = |user_id: &str, role: SpaceRole| -> Result<SpaceMember, DbError> {
        Ok(conn.query_row(
            "SELECT username, display_name, share_public FROM users WHERE id = ?1",
            [user_id],
            |row| {
                Ok(SpaceMember {
                    user_id: Some(user_id.to_owned()),
                    group_id: None,
                    name: row.get(0)?,
                    display_name: Some(row.get(1)?),
                    role,
                    public_key: row.get(2)?,
                })
            },
        )?)
    };
    let mut out = vec![person(&space.owner_id, SpaceRole::Owner)?];
    let mut stmt = conn.prepare(
        "SELECT m.user_id, m.group_id, m.role, g.name FROM sync_members m
         LEFT JOIN groups g ON g.id = m.group_id WHERE m.space_id = ?1",
    )?;
    let rows = stmt
        .query_map([&space.id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut rest = Vec::new();
    for (user, group, role, group_name) in rows {
        let role = role_of_text(&role);
        match (user, group) {
            (Some(user), _) => rest.push(person(&user, role)?),
            (None, Some(group)) => rest.push(SpaceMember {
                user_id: None,
                group_id: Some(group),
                name: group_name.unwrap_or_default(),
                display_name: None,
                role,
                public_key: None,
            }),
            (None, None) => {}
        }
    }
    rest.sort_by(|a, b| a.name.cmp(&b.name));
    out.extend(rest);
    Ok(out)
}

/// An account to share with, by username.
///
/// # Errors
///
/// Database errors only.
pub fn person(conn: &Connection, username: &str) -> Result<Option<Person>, DbError> {
    Ok(conn
        .query_row(
            "SELECT id, username, display_name, kind, share_public FROM users
             WHERE username = ?1 AND disabled_at IS NULL",
            [username.trim().to_lowercase()],
            |row| {
                Ok(Person {
                    user_id: row.get(0)?,
                    username: row.get(1)?,
                    display_name: row.get(2)?,
                    kind: crate::accounts::e2ee::kind_of(&row.get::<_, String>(3)?),
                    public_key: row.get(4)?,
                })
            },
        )
        .optional()?)
}

fn public_key(conn: &Connection, user_id: &str) -> Result<Option<String>, DbError> {
    Ok(conn
        .query_row(
            "SELECT share_public FROM users WHERE id = ?1",
            [user_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten())
}

/// An account's key pair for sharing, if a device made one.
///
/// # Errors
///
/// Database errors only.
pub fn key_pair(conn: &Connection, user_id: &str) -> Result<Option<AccountKeyPair>, DbError> {
    let found: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT share_public, share_private FROM users WHERE id = ?1",
            [user_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(match found {
        Some((Some(public_key), Some(private_sealed))) => Some(AccountKeyPair {
            public_key,
            private_sealed,
        }),
        _ => None,
    })
}

/// Keeps an account's key pair, once: keys were sealed to the first one,
/// a second would leave them unreadable. Returns the pair kept, the one
/// sent when there was none.
///
/// # Errors
///
/// Database errors only.
pub fn set_key_pair(
    conn: &Connection,
    user_id: &str,
    pair: &AccountKeyPair,
) -> Result<AccountKeyPair, DbError> {
    conn.execute(
        "UPDATE users SET share_public = ?2, share_private = ?3
         WHERE id = ?1 AND share_public IS NULL",
        params![user_id, pair.public_key, pair.private_sealed],
    )?;
    Ok(key_pair(conn, user_id)?.unwrap_or_else(|| pair.clone()))
}
