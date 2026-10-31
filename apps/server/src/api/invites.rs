// SPDX-License-Identifier: AGPL-3.0-or-later

//! Invite links. Admins can always invite. Everyone else can when
//! registration is set to `invite`.

use app_protocol::account::{CreateInviteRequest, Invite, Registration};
use app_protocol::error_code;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use super::extract::{Body, ClientIp, CurrentUser};
use crate::accounts::store::{self, Event, InviteRow};
use crate::clock::{DAY, HOUR};
use crate::error::Error;
use crate::routes::AppState;

/// Longest invite a person can ask for.
const MAX_DAYS: u32 = 30;
/// Unused invites a member may hold at once. Admins have no cap: with
/// "invite" sign-up, a member without one, or a stolen member session,
/// could otherwise make accounts without end.
const MEMBER_PENDING: i64 = 10;

fn invite_json(row: InviteRow) -> Invite {
    Invite {
        id: row.id,
        token: None,
        link: None,
        created_by: row.created_by,
        created_at: row.created_at,
        expires_at: row.expires_at,
        state: row.state.into(),
    }
}

/// `POST /api/v1/invites`
pub async fn create(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<CreateInviteRequest>,
) -> Result<(StatusCode, Json<Invite>), Error> {
    let now = state.clock.now();
    let life = match req.hours {
        Some(hours) => i64::from(hours.clamp(1, 24)) * HOUR,
        None => {
            i64::from(
                req.days
                    .unwrap_or(state.config.auth.invite_days)
                    .clamp(1, MAX_DAYS),
            ) * DAY
        }
    };
    let ip_text = ip.0.to_string();
    let user_id = me.user.id.clone();
    let is_admin = me.user.acts_as_admin();
    let created = state
        .db
        .call(move |c| {
            if !is_admin && store::registration(c)? != Registration::Invite {
                return Ok(Err(Error::forbidden()));
            }
            if !is_admin {
                let pending: i64 = c.query_row(
                    "SELECT count(*) FROM invites WHERE created_by = ?1
                     AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?2",
                    rusqlite::params![user_id, now],
                    |row| row.get(0),
                )?;
                if pending >= MEMBER_PENDING {
                    return Ok(Err(Error::new(
                        StatusCode::CONFLICT,
                        app_protocol::error_code::INVALID,
                        format!(
                            "{MEMBER_PENDING} invites wait to be used already: revoke one first"
                        ),
                    )));
                }
            }
            let (id, plain, expires_at) = store::create_invite(c, &user_id, now, life)?;
            store::audit(
                c,
                now,
                &Event {
                    name: "invite_created",
                    user_id: Some(&user_id),
                    ip: Some(&ip_text),
                    detail: Some(&id),
                    ..Event::default()
                },
            )?;
            Ok(Ok((id, plain, expires_at)))
        })
        .await?;
    let (id, plain, expires_at) = created?;
    let base = state.config.server.public_url.trim_end_matches('/');
    Ok((
        StatusCode::CREATED,
        Json(Invite {
            id,
            link: (!base.is_empty()).then(|| format!("{base}/invite/{plain}")),
            token: Some(plain),
            created_by: me.user.username,
            created_at: now,
            expires_at,
            state: "pending".into(),
        }),
    ))
}

/// `GET /api/v1/invites`: mine, or every invite for an admin.
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<Vec<Invite>>, Error> {
    let now = state.clock.now();
    let only = (!me.user.acts_as_admin()).then(|| me.user.id.clone());
    let rows = state
        .db
        .call(move |c| store::list_invites(c, only.as_deref(), now))
        .await?;
    Ok(Json(rows.into_iter().map(invite_json).collect()))
}

/// `DELETE /api/v1/invites/{id}`: one of mine, or any for an admin.
pub async fn revoke(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let only = (!me.user.acts_as_admin()).then(|| me.user.id.clone());
    let done = state
        .db
        .call(move |c| {
            let done = store::revoke_invite(c, &id, only.as_deref(), now)?;
            if done {
                store::audit(
                    c,
                    now,
                    &Event {
                        name: "invite_revoked",
                        user_id: Some(&me.user.id),
                        ip: Some(&ip_text),
                        detail: Some(&id),
                        ..Event::default()
                    },
                )?;
            }
            Ok(done)
        })
        .await?;
    if done {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Error::new(
            StatusCode::NOT_FOUND,
            error_code::NOT_FOUND,
            "no pending invite with this id",
        ))
    }
}
