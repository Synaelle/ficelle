// SPDX-License-Identifier: AGPL-3.0-or-later

//! The sharing endpoints: shared spaces and who is in them, the keys an
//! owner's devices seal for members of an e2ee space, and e2ee accounts'
//! key pairs. Only the owner changes who is in a space or stops sharing
//! it, a member can leave. Every account in a space hears of a change
//! through its live sockets.

use app_protocol::e2ee::AccountKind;
use app_protocol::error_code;
use app_protocol::sync::{
    AccountKeyPair, GiveGrants, Person, ShareRequest, SharedSpace, SharedSpaces, SpaceMembers,
    SpaceRole,
};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use super::extract::{Body, CurrentUser};
use crate::accounts::store::Authed;
use crate::error::Error;
use crate::routes::AppState;
use crate::sharing::{self as store, Shared};

fn no_space() -> Error {
    Error::new(
        StatusCode::NOT_FOUND,
        error_code::NOT_FOUND,
        "no such shared space",
    )
}

fn owners_only() -> Error {
    Error::new(
        StatusCode::FORBIDDEN,
        error_code::FORBIDDEN,
        "only the owner changes who it is shared with",
    )
}

/// The space and the account's role in it, not found when it is not in it.
async fn space_for(
    state: &AppState,
    me: &Authed,
    id: String,
) -> Result<(Shared, SpaceRole), Error> {
    let user = me.user.id.clone();
    state
        .db
        .call(move |c| {
            let Some(space) = store::shared(c, &id)? else {
                return Ok(None);
            };
            Ok(store::role_in(c, &space, &user)?.map(|role| (space, role)))
        })
        .await?
        .ok_or_else(no_space)
}

async fn owned(state: &AppState, me: &Authed, id: String) -> Result<Shared, Error> {
    match space_for(state, me, id).await? {
        (space, SpaceRole::Owner) => Ok(space),
        _ => Err(owners_only()),
    }
}

/// `GET /api/v1/sync/spaces`
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<SharedSpaces>, Error> {
    let id = me.user.id.clone();
    let spaces = state.db.call(move |c| store::spaces_of(c, &id)).await?;
    Ok(Json(SharedSpaces { spaces }))
}

/// `POST /api/v1/sync/spaces`
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Body(req): Body<ShareRequest>,
) -> Result<(StatusCode, Json<SharedSpace>), Error> {
    let now = state.clock.now();
    let master = state.master.clone();
    let (owner, kind) = (me.user.id.clone(), me.user.kind);
    let (space, touched) = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let made = match store::create(&tx, &master, &owner, kind, &req.members, now)? {
                Ok(made) => made,
                Err(why) => return Ok(Err(why)),
            };
            let space = store::spaces_of(&tx, &owner)?
                .into_iter()
                .find(|s| s.id == made.0);
            tx.commit()?;
            Ok(Ok((space, made.1)))
        })
        .await?
        .map_err(Error::invalid)?;
    state.live.spaces_changed(&touched);
    let space = space.ok_or_else(no_space)?;
    Ok((StatusCode::CREATED, Json(space)))
}

/// `GET /api/v1/sync/spaces/{id}/members`
pub async fn members(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<SpaceMembers>, Error> {
    let (space, _) = space_for(&state, &me, id).await?;
    let members = state.db.call(move |c| store::members(c, &space)).await?;
    Ok(Json(SpaceMembers { members }))
}

/// `PUT /api/v1/sync/spaces/{id}/members`
pub async fn set_members(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
    Body(req): Body<ShareRequest>,
) -> Result<Json<SpaceMembers>, Error> {
    let space = owned(&state, &me, id).await?;
    let now = state.clock.now();
    let (members, touched) = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let touched = match store::set_members(&tx, &space, &req.members, now)? {
                Ok(touched) => touched,
                Err(why) => return Ok(Err(why)),
            };
            let members = store::members(&tx, &space)?;
            tx.commit()?;
            Ok(Ok((members, touched)))
        })
        .await?
        .map_err(Error::invalid)?;
    state.live.spaces_changed(&touched);
    Ok(Json(SpaceMembers { members }))
}

/// `DELETE /api/v1/sync/spaces/{id}`: the owner stops sharing. Members'
/// devices drop it, the owner's devices moved what it held back first.
pub async fn stop(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    let space = owned(&state, &me, id).await?;
    let touched = state.db.call(move |c| store::delete(c, &space)).await?;
    state.live.spaces_changed(&touched);
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/v1/sync/spaces/{id}/me`: a member leaves.
pub async fn leave(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    let (space, role) = space_for(&state, &me, id).await?;
    if role == SpaceRole::Owner {
        return Err(Error::invalid(
            "the owner stops sharing rather than leaving",
        ));
    }
    let user = me.user.id.clone();
    let touched = state
        .db
        .call(move |c| store::leave(c, &space, &user))
        .await?;
    state.live.spaces_changed(&touched);
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/sync/spaces/{id}/grants`: keys sealed by one of the
/// owner's devices.
pub async fn give(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
    Body(req): Body<GiveGrants>,
) -> Result<StatusCode, Error> {
    let space = owned(&state, &me, id).await?;
    if space.kind != AccountKind::E2ee {
        return Err(Error::invalid("keys are for end-to-end encrypted spaces"));
    }
    if req.grants.len() > 1000
        || req
            .grants
            .iter()
            .any(|g| g.grant.is_empty() || g.grant.len() > 1024)
    {
        return Err(Error::invalid("a key is 1 to 1024 characters"));
    }
    let grants: Vec<(String, String)> = req
        .grants
        .into_iter()
        .map(|g| (g.user_id, g.grant))
        .collect();
    let got = state
        .db
        .call(move |c| store::give(c, &space, req.epoch, &grants))
        .await?
        .map_err(Error::invalid)?;
    state.live.spaces_changed(&got);
    Ok(StatusCode::NO_CONTENT)
}

/// Whom to look up.
#[derive(Debug, Deserialize)]
pub struct PersonQuery {
    name: String,
}

/// `GET /api/v1/sync/people?name=`: an account to share with, by its exact
/// username, so the list of accounts cannot be read through it.
pub async fn person(
    State(state): State<AppState>,
    CurrentUser(_): CurrentUser,
    Query(q): Query<PersonQuery>,
) -> Result<Json<Person>, Error> {
    let found = state.db.call(move |c| store::person(c, &q.name)).await?;
    found.map(Json).ok_or_else(|| {
        Error::new(
            StatusCode::NOT_FOUND,
            error_code::NOT_FOUND,
            "no such account",
        )
    })
}

/// `GET /api/v1/sync/key-pair`
pub async fn key_pair(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<AccountKeyPair>, Error> {
    let id = me.user.id.clone();
    state
        .db
        .call(move |c| store::key_pair(c, &id))
        .await?
        .map(Json)
        .ok_or_else(|| {
            Error::new(
                StatusCode::NOT_FOUND,
                error_code::NOT_FOUND,
                "no key pair yet",
            )
        })
}

/// `PUT /api/v1/sync/key-pair`: the first device of an e2ee account sends
/// the pair it made. Once there, it stays: the answer is the pair kept,
/// and a device that raced another takes that one.
pub async fn set_key_pair(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Body(pair): Body<AccountKeyPair>,
) -> Result<Json<AccountKeyPair>, Error> {
    if me.user.kind != AccountKind::E2ee {
        return Err(Error::invalid(
            "key pairs belong to end-to-end encrypted accounts",
        ));
    }
    if !app_e2ee::public_key_ok(&pair.public_key)
        || pair.private_sealed.is_empty()
        || pair.private_sealed.len() > 1024
    {
        return Err(Error::invalid("this is not a key pair"));
    }
    let id = me.user.id.clone();
    let kept = state
        .db
        .call(move |c| store::set_key_pair(c, &id, &pair))
        .await?;
    Ok(Json(kept))
}
