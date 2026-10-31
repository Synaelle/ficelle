// SPDX-License-Identifier: AGPL-3.0-or-later

//! The sync endpoints: the account's space, pushing changes, pulling them.
//! `space=` on a call names a shared space the account is in instead, see
//! [`super::sharing`].

use std::collections::HashMap;

use app_protocol::e2ee::AccountKind;
use app_protocol::error_code;
use app_protocol::sync::{
    DocKey, HeavyDoc, HeavyDocs, IncomingChange, PullAnswer, PushAnswer, PushRequest, SpaceInfo,
    SpaceRole,
};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;

use super::extract::{Body, CurrentUser};
use crate::accounts::store::Authed;
use crate::error::Error;
use crate::routes::AppState;
use crate::sync::{self as store, NewChange, Space};

/// Largest single change. A whole note with long text is far below.
const MAX_CHANGE: usize = 8 * 1024 * 1024;
/// Changes per pull page, by default and at most.
const PAGE: usize = 500;
const MAX_PAGE: usize = 2000;

fn invalid(why: &str) -> Error {
    Error::new(StatusCode::BAD_REQUEST, error_code::CHANGE_INVALID, why)
}

/// `space` or a note id: lower case letters, digits and dashes.
fn doc_id_ok(doc: &str) -> bool {
    doc == "space" || (doc.len() == 36 && doc.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
}

fn quota_full() -> Error {
    Error::new(
        StatusCode::INSUFFICIENT_STORAGE,
        error_code::QUOTA_FULL,
        "the storage of this space's owner is full",
    )
}

fn change_id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'))
}

async fn space(state: &AppState, me: &Authed) -> Result<Space, Error> {
    let now = state.clock.now();
    let master = state.master.clone();
    let (id, kind) = (me.user.id.clone(), me.user.kind);
    Ok(state
        .db
        .call(move |c| store::space_of(c, &master, &id, kind, now))
        .await?)
}

/// `space=`: a shared space instead of the account's own.
#[derive(Debug, Default, Deserialize)]
pub struct InSpace {
    #[serde(default)]
    space: Option<String>,
}

/// The space a call is for, the account's role in it, and everyone in it
/// for a shared one (to tell them of a push).
pub(super) struct Target {
    pub(super) space: Space,
    pub(super) role: SpaceRole,
    pub(super) people: Vec<String>,
}

fn no_space() -> Error {
    Error::new(
        StatusCode::NOT_FOUND,
        error_code::NOT_FOUND,
        "no such shared space",
    )
}

pub(super) async fn target(
    state: &AppState,
    me: &Authed,
    wanted: Option<String>,
) -> Result<Target, Error> {
    let Some(id) = wanted.filter(|id| *id != me.user.id) else {
        return Ok(Target {
            space: space(state, me).await?,
            role: SpaceRole::Owner,
            people: Vec::new(),
        });
    };
    let user = me.user.id.clone();
    state
        .db
        .call(move |c| {
            let Some(shared) = crate::sharing::shared(c, &id)? else {
                return Ok(None);
            };
            let people = crate::sharing::people(c, &shared)?;
            let Some(role) = people.iter().find(|(p, _)| *p == user).map(|(_, r)| *r) else {
                return Ok(None);
            };
            let data_key = store::data_key(c, &id)?;
            Ok(Some(Target {
                space: Space {
                    id,
                    kind: shared.kind,
                    data_key,
                },
                role,
                people: people.into_iter().map(|(p, _)| p).collect(),
            }))
        })
        .await?
        .ok_or_else(no_space)
}

/// `GET /api/v1/sync`
pub async fn info(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<SpaceInfo>, Error> {
    let space = space(&state, &me).await?;
    let id = space.id.clone();
    let head = state.db.call(move |c| store::head(c, &id)).await?;
    let coediting = state.db.call(|c| store::coediting(c)).await?;
    Ok(Json(SpaceInfo {
        space_id: space.id,
        kind: space.kind,
        head,
        coediting,
    }))
}

/// Each change's bytes, checked: ids, documents, sizes and keys.
fn decode(req: &PushRequest, kind: AccountKind, shared: bool) -> Result<Vec<Vec<u8>>, Error> {
    let mut decoded = Vec::with_capacity(req.changes.len());
    for change in &req.changes {
        if !change_id_ok(&change.id) {
            return Err(invalid("a change id is 1 to 64 of a-z, A-Z, 0-9, - _ :"));
        }
        if !doc_id_ok(&change.doc) {
            return Err(invalid("unknown document id"));
        }
        let bytes = STANDARD
            .decode(&change.data)
            .map_err(|_| invalid("change data is not base64"))?;
        if bytes.is_empty() || bytes.len() > MAX_CHANGE {
            return Err(invalid("a change is 1 byte to 8 MB"));
        }
        match (kind, &change.key) {
            (AccountKind::Standard, Some(_)) => {
                return Err(invalid("document keys belong to e2ee accounts"));
            }
            // a shared e2ee space's changes are sealed with its own key
            (AccountKind::E2ee, Some(_)) if shared => {
                return Err(invalid("a shared space's changes carry no document key"));
            }
            (AccountKind::E2ee, Some(key)) if key.is_empty() || key.len() > 1024 => {
                return Err(invalid("a document key is 1 to 1024 characters"));
            }
            _ => {}
        }
        decoded.push(bytes);
    }
    Ok(decoded)
}

/// `POST /api/v1/sync/push`
pub async fn push(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Query(within): Query<InSpace>,
    Body(req): Body<PushRequest>,
) -> Result<Json<PushAnswer>, Error> {
    let Target {
        space,
        role,
        people,
    } = target(&state, &me, within.space).await?;
    if role == SpaceRole::Read {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            error_code::FORBIDDEN,
            "this is shared with you to read only",
        ));
    }
    let shared = !people.is_empty();
    let space_id = space.id.clone();
    let decoded = decode(&req, space.kind, shared)?;
    let now = state.clock.now();
    let master = state.master.clone();
    let device = me.device_id.clone();
    let total = u32::try_from(req.changes.len()).unwrap_or(u32::MAX);
    let files_config = state.config.files.clone();
    let (accepted, head) = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            // a compaction replaces many changes with one and frees room:
            // refusing it at a full quota would keep the space full for
            // good. It is let through up to the bytes it empties, once per
            // document, not more
            let mut freed: HashMap<&str, u64> = HashMap::new();
            let mut growing: u64 = 0;
            for (change, bytes) in req.changes.iter().zip(&decoded) {
                let mut size = bytes.len() as u64;
                if let Some(up_to) = change.compacts
                    && !freed.contains_key(change.doc.as_str())
                {
                    let room = store::compaction_frees(&tx, &space.id, &change.doc, up_to)?;
                    freed.insert(&change.doc, room);
                }
                if change.compacts.is_some()
                    && let Some(room) = freed.get_mut(change.doc.as_str())
                {
                    let taken = size.min(*room);
                    *room -= taken;
                    size -= taken;
                }
                growing += size;
            }
            if growing > 0
                && crate::files::room_in(&tx, &files_config, &space.id)?
                    .is_some_and(|room| growing > room)
            {
                return Ok(Err(quota_full()));
            }
            let changes: Vec<NewChange<'_>> = req
                .changes
                .iter()
                .zip(&decoded)
                .map(|(change, bytes)| NewChange {
                    change_id: &change.id,
                    doc_id: &change.doc,
                    bytes,
                    key: change.key.as_deref(),
                    compacts: change.compacts,
                })
                .collect();
            let before = store::head(&tx, &space.id)?;
            if changes
                .iter()
                .any(|c| c.compacts.is_some_and(|n| n < 1 || n > before))
            {
                return Ok(Err(invalid(
                    "a compaction names a change this space does not have",
                )));
            }
            let accepted = store::push(&tx, &master, &space, &device, &changes, now)?;
            let head = store::head(&tx, &space.id)?;
            tx.commit()?;
            Ok(Ok((accepted, head)))
        })
        .await??;
    if accepted > 0 {
        if shared {
            for person in &people {
                state
                    .live
                    .changed(person, head, &me.device_id, Some(&space_id));
            }
        } else {
            state.live.changed(&space_id, head, &me.device_id, None);
        }
    }
    Ok(Json(PushAnswer {
        accepted,
        duplicates: total - accepted,
        head,
    }))
}

/// Least changes for [`heavy`], by default.
const HEAVY: i64 = 200;

/// How many changes make a document heavy.
#[derive(Debug, Deserialize)]
pub struct HeavyQuery {
    #[serde(default)]
    min: Option<i64>,
    #[serde(default)]
    space: Option<String>,
}

/// `GET /api/v1/sync/heavy?min=`
pub async fn heavy(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Query(q): Query<HeavyQuery>,
) -> Result<Json<HeavyDocs>, Error> {
    let space = target(&state, &me, q.space).await?.space;
    let min = q.min.unwrap_or(HEAVY).max(2);
    let docs = state
        .db
        .call(move |c| store::heavy(c, &space.id, min))
        .await?;
    Ok(Json(HeavyDocs {
        docs: docs
            .into_iter()
            .map(|(doc, changes)| HeavyDoc { doc, changes })
            .collect(),
    }))
}

/// Where a pull starts and how much it takes.
#[derive(Debug, Deserialize)]
pub struct PullQuery {
    #[serde(default)]
    after: i64,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    space: Option<String>,
}

/// `GET /api/v1/sync/changes?after=&limit=`
pub async fn pull(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Query(q): Query<PullQuery>,
) -> Result<Json<PullAnswer>, Error> {
    let space = target(&state, &me, q.space).await?.space;
    let limit = q.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE);
    let master = state.master.clone();
    let after = q.after.max(0);
    let (changes, more, keys, head, anchor) = state
        .db
        .call(move |c| {
            let (changes, more) = store::pull(c, &master, &space, after, limit)?;
            let keys = if space.kind == AccountKind::E2ee {
                let mut docs: Vec<&str> = changes.iter().map(|ch| ch.doc_id.as_str()).collect();
                docs.sort_unstable();
                docs.dedup();
                store::doc_keys(c, &space.id, &docs)?
            } else {
                Vec::new()
            };
            let head = store::head(c, &space.id)?;
            let anchor = if after > 0 {
                store::change_at(c, &space.id, after)?
            } else {
                None
            };
            Ok((changes, more, keys, head, anchor))
        })
        .await?;
    Ok(Json(PullAnswer {
        changes: changes
            .into_iter()
            .map(|ch| IncomingChange {
                seq: ch.seq,
                id: ch.change_id,
                doc: ch.doc_id,
                data: STANDARD.encode(ch.bytes.as_slice()),
                device: ch.device_id,
            })
            .collect(),
        keys: keys
            .into_iter()
            .map(|(doc, wrapped)| DocKey { doc, wrapped })
            .collect(),
        head,
        more,
        anchor,
    }))
}

/// How often a live socket is pinged and its session checked again.
#[cfg(not(test))]
const LIVE_CHECK: std::time::Duration = std::time::Duration::from_secs(30);
#[cfg(test)]
const LIVE_CHECK: std::time::Duration = std::time::Duration::from_millis(300);

/// `GET /api/v1/sync/live`, upgraded to a WebSocket: notices that the space,
/// or a shared space the account is in, changed. The session is checked again every 30 seconds, a device signed
/// out or a token run out closes the socket (code 4401) and the device
/// comes back with a fresh token.
pub async fn live(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    headers: axum::http::HeaderMap,
    ws: axum::extract::WebSocketUpgrade,
) -> Result<axum::response::Response, Error> {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| crate::accounts::token::hash(t.trim()))
        .ok_or_else(Error::unauthorized)?;
    let space = space(&state, &me).await?;
    let id = space.id.clone();
    let (head, coediting) = state
        .db
        .call(move |c| Ok((store::head(c, &id)?, store::coediting(c)?)))
        .await?;
    Ok(ws.on_upgrade(move |socket| live_socket(socket, state, bearer, me, head, coediting)))
}

/// Tells everyone in these shared spaces who views their notes now.
pub(super) async fn announce(state: &AppState, spaces: Vec<String>) {
    for space in spaces {
        let id = space.clone();
        let people = state
            .db
            .call(move |c| match crate::sharing::shared(c, &id)? {
                Some(shared) => crate::sharing::people(c, &shared),
                None => Ok(Vec::new()),
            })
            .await
            .unwrap_or_default();
        let viewers = state.live.viewers(&space);
        for (person, _) in people {
            state.live.presence(&person, &space, &viewers);
        }
    }
}

/// "Viewing" messages a live socket may send in a minute.
const VIEWING_PER_MINUTE: u32 = 30;

/// A socket seated in a space its account was taken out of leaves it: the
/// others stop seeing someone who is no longer there.
async fn leave_if_out(state: &AppState, me: &Authed, seat: u64, seated: &mut Option<String>) {
    let Some(space) = seated.clone() else {
        return;
    };
    let user = me.user.id.clone();
    let id = space.clone();
    let member = state
        .db
        .call(move |c| {
            Ok(match crate::sharing::shared(c, &id)? {
                Some(shared) => crate::sharing::role_in(c, &shared, &user)?.is_some(),
                None => false,
            })
        })
        .await
        .unwrap_or(false);
    if !member {
        *seated = None;
        let changed = state.live.view(seat, None);
        announce(state, changed).await;
    }
}

/// What a socket says it views, kept when it is a note of a shared space
/// the account is in and the admin allows co-editing.
async fn viewing(
    state: &AppState,
    me: &Authed,
    request: app_protocol::sync::LiveRequest,
) -> Option<(String, app_protocol::sync::Viewer)> {
    let app_protocol::sync::LiveRequest::Viewing {
        space: Some(space),
        note: Some(note),
    } = request
    else {
        return None;
    };
    if !doc_id_ok(&note) || note == "space" || space.len() > 64 {
        return None;
    }
    let user = me.user.id.clone();
    let id = space.clone();
    let allowed = state
        .db
        .call(move |c| {
            if !store::coediting(c)? {
                return Ok(false);
            }
            Ok(match crate::sharing::shared(c, &id)? {
                Some(shared) => crate::sharing::role_in(c, &shared, &user)?.is_some(),
                None => false,
            })
        })
        .await
        .unwrap_or(false);
    allowed.then(|| {
        (
            space,
            app_protocol::sync::Viewer {
                name: me.user.username.clone(),
                display_name: me.user.display_name.clone(),
                note,
            },
        )
    })
}

async fn live_socket(
    mut socket: axum::extract::ws::WebSocket,
    state: AppState,
    bearer: crate::accounts::token::TokenHash,
    me: Authed,
    head: i64,
    coediting: bool,
) {
    use app_protocol::sync::LiveMessage;
    use axum::extract::ws::{CloseFrame, Message};
    use tokio::sync::broadcast::error::RecvError;

    use crate::live::Notice;

    let device = me.device_id.clone();
    let seat = state.live.socket_id();
    let mut notices = state.live.subscribe(&me.user.id);
    let send = |message: &LiveMessage| {
        Message::Text(serde_json::to_string(message).unwrap_or_default().into())
    };
    if socket
        .send(send(&LiveMessage::Hello { head, coediting }))
        .await
        .is_err()
    {
        return;
    }
    let mut check = tokio::time::interval(LIVE_CHECK);
    check.tick().await;
    // the space this socket views a note of, and its recent "viewing" messages
    let mut seated: Option<String> = None;
    let mut asked = (0u32, state.clock.now());
    loop {
        let out = tokio::select! {
            notice = notices.recv() => match notice {
                Ok(Notice::Changed { head, device: from, space }) if from != device => {
                    // spaces changed (head 0): one this socket sits in may be
                    // closed to it now
                    if head == 0 {
                        leave_if_out(&state, &me, seat, &mut seated).await;
                    }
                    Some(send(&LiveMessage::Changed { head, space }))
                }
                Ok(Notice::Changed { .. }) => None,
                Ok(Notice::Settings { coediting }) => Some(send(&LiveMessage::Settings { coediting })),
                Ok(Notice::Presence { space, viewers }) => {
                    Some(send(&LiveMessage::Presence { space, viewers }))
                }
                // fell behind: one pull covers everything missed
                Err(RecvError::Lagged(_)) => Some(send(&LiveMessage::Changed { head: 0, space: None })),
                Err(RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(Message::Text(text))) => {
                    // a person moves between notes a few times a minute: a
                    // socket that sends far more only floods the others
                    let now = state.clock.now();
                    if now - asked.1 > crate::clock::MINUTE {
                        asked = (0, now);
                    }
                    asked.0 += 1;
                    if asked.0 > VIEWING_PER_MINUTE {
                        let _ = socket
                            .send(Message::Close(Some(CloseFrame {
                                code: 4429,
                                reason: "too many messages".into(),
                            })))
                            .await;
                        break;
                    }
                    if let Ok(request) = serde_json::from_str(&text) {
                        let seen = viewing(&state, &me, request).await;
                        seated = seen.as_ref().map(|(space, _)| space.clone());
                        let changed = state.live.view(seat, seen);
                        announce(&state, changed).await;
                    }
                    None
                }
                Some(Ok(_)) => None,
            },
            _ = check.tick() => {
                let hash = bearer;
                let now = state.clock.now();
                let valid = state
                    .db
                    .call(move |c| crate::accounts::store::authenticate(c, &hash, now))
                    .await
                    .is_ok_and(|a| a.is_some());
                if !valid {
                    let _ = socket
                        .send(Message::Close(Some(CloseFrame {
                            code: 4401,
                            reason: "sign in again".into(),
                        })))
                        .await;
                    break;
                }
                leave_if_out(&state, &me, seat, &mut seated).await;
                Some(Message::Ping(Vec::new().into()))
            }
        };
        if let Some(message) = out
            && socket.send(message).await.is_err()
        {
            break;
        }
    }
    // gone: the others stop seeing this device in the note
    let changed = state.live.view(seat, None);
    announce(&state, changed).await;
}
