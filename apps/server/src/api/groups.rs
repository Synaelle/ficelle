// SPDX-License-Identifier: AGPL-3.0-or-later

//! Groups. Admins create them and choose their members, everyone sees the
//! groups they are in.

use app_protocol::admin::{Group, GroupDetail, GroupRequest};
use app_protocol::error_code;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use super::admin::{admin, admin_event};
use super::extract::{Body, ClientIp, CurrentUser};
use crate::accounts::store;
use crate::admin as admin_store;
use crate::error::Error;
use crate::routes::AppState;

fn no_group() -> Error {
    Error::new(
        StatusCode::NOT_FOUND,
        error_code::NOT_FOUND,
        "no such group",
    )
}

fn name_of(raw: &str) -> Result<String, Error> {
    admin_store::group_name(raw).ok_or_else(|| Error::invalid("use 1 to 64 characters"))
}

fn name_taken() -> Error {
    Error::new(
        StatusCode::CONFLICT,
        error_code::GROUP_NAME_TAKEN,
        "another group has this name",
    )
}

/// `GET /api/v1/me/groups`
pub async fn mine(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<Vec<Group>>, Error> {
    let id = me.user.id.clone();
    Ok(Json(
        state
            .db
            .call(move |c| admin_store::groups_of(c, &id))
            .await?,
    ))
}

/// `GET /api/v1/admin/groups`
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<Vec<Group>>, Error> {
    admin(&me)?;
    Ok(Json(state.db.call(|c| admin_store::list_groups(c)).await?))
}

/// `POST /api/v1/admin/groups`
pub async fn create(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<GroupRequest>,
) -> Result<(StatusCode, Json<Group>), Error> {
    admin(&me)?;
    let name = name_of(&req.name)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let group = state
        .db
        .call(move |c| {
            let Some(group) = admin_store::create_group(c, &name, now)? else {
                return Ok(None);
            };
            admin_event(c, now, "group_created", None, &me, &ip_text, &group.name)?;
            Ok(Some(group))
        })
        .await?
        .ok_or_else(name_taken)?;
    Ok((StatusCode::CREATED, Json(group)))
}

/// `GET /api/v1/admin/groups/{id}`
pub async fn detail(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<GroupDetail>, Error> {
    admin(&me)?;
    let found = state
        .db
        .call(move |c| {
            let Some(group) = admin_store::group(c, &id)? else {
                return Ok(None);
            };
            Ok(Some(GroupDetail {
                group,
                member_list: admin_store::members(c, &id)?,
            }))
        })
        .await?;
    found.map(Json).ok_or_else(no_group)
}

/// `PATCH /api/v1/admin/groups/{id}`: renames.
pub async fn rename(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
    Body(req): Body<GroupRequest>,
) -> Result<Json<Group>, Error> {
    admin(&me)?;
    let name = name_of(&req.name)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let group = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let Some(before) = admin_store::group(&tx, &id)? else {
                return Ok(Err(no_group()));
            };
            if admin_store::group_name_taken(&tx, &name, Some(&id))? {
                return Ok(Err(name_taken()));
            }
            admin_store::rename_group(&tx, &id, &name)?;
            let what = format!("{} to {name}", before.name);
            admin_event(&tx, now, "group_renamed", None, &me, &ip_text, &what)?;
            let after = admin_store::group(&tx, &id)?;
            tx.commit()?;
            Ok(after.ok_or_else(no_group))
        })
        .await??;
    Ok(Json(group))
}

/// `DELETE /api/v1/admin/groups/{id}`: the group, not its members.
pub async fn delete(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let (found, touched) = state
        .db
        .call(move |c| {
            let Some(group) = admin_store::group(c, &id)? else {
                return Ok((false, Vec::new()));
            };
            // spaces shared with the group lose everyone in it
            let ((), touched) = crate::sharing::around_group(c, &id, |c| {
                admin_store::delete_group(c, &id).map(drop)
            })?;
            admin_event(c, now, "group_deleted", None, &me, &ip_text, &group.name)?;
            Ok((true, touched))
        })
        .await?;
    state.live.spaces_changed(&touched);
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(no_group())
    }
}

/// `PUT /api/v1/admin/groups/{id}/members/{user}`
pub async fn add_member(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let touched = state
        .db
        .call(move |c| {
            let Some(group) = admin_store::group(c, &id)? else {
                return Ok(Err(no_group()));
            };
            if store::user_by_id(c, &user_id)?.is_none() {
                return Ok(Err(Error::new(
                    StatusCode::NOT_FOUND,
                    error_code::NOT_FOUND,
                    "no such account",
                )));
            }
            let ((), touched) = crate::sharing::around_group(c, &id, |c| {
                admin_store::add_member(c, &id, &user_id, now)
            })?;
            admin_event(
                c,
                now,
                "group_member_added",
                Some(&user_id),
                &me,
                &ip_text,
                &group.name,
            )?;
            Ok(Ok(touched))
        })
        .await??;
    // they are in the spaces shared with the group now
    state.live.spaces_changed(&touched);
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/v1/admin/groups/{id}/members/{user}`
pub async fn remove_member(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let (removed, touched) = state
        .db
        .call(move |c| {
            let Some(group) = admin_store::group(c, &id)? else {
                return Ok((false, Vec::new()));
            };
            let (removed, touched) = crate::sharing::around_group(c, &id, |c| {
                admin_store::remove_member(c, &id, &user_id)
            })?;
            if removed {
                admin_event(
                    c,
                    now,
                    "group_member_removed",
                    Some(&user_id),
                    &me,
                    &ip_text,
                    &group.name,
                )?;
            }
            Ok((removed, touched))
        })
        .await?;
    state.live.spaces_changed(&touched);
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Error::new(
            StatusCode::NOT_FOUND,
            error_code::NOT_FOUND,
            "not a member",
        ))
    }
}
