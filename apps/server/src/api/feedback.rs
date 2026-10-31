// SPDX-License-Identifier: AGPL-3.0-or-later

//! Feedback from anyone's apps (`/feedback/*`, no account, any origin), the
//! moderation inbox (admins, the readers they name, and a read-only token
//! for `reports pull`), and the admin's switches. The report routes read no
//! session: a signed-in person's report is as anonymous as anyone's.

use std::time::Duration;

use app_protocol::error_code;
use app_protocol::feedback::{Challenge, FeedbackInfo, Report};
use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::admin::{admin, admin_event};
use super::extract::{Body, ClientIp, CurrentUser};
use crate::accounts::store::{self, Authed};
use crate::accounts::token;
use crate::clock::DAY;
use crate::db::DbError;
use crate::error::Error;
use crate::feedback::{self, BlockKind, Caps, Outcome, Settings, inbox};
use crate::routes::AppState;

/// Largest report request: three images of 2 MB in base64, the logs and
/// the words.
pub const MAX_REQUEST: usize = 9 * 1024 * 1024;

/// A report's body must arrive within this, a slow sender holds nothing
/// for long.
const READ_TIME: Duration = Duration::from_secs(30);

async fn current(state: &AppState) -> Result<Settings, Error> {
    let config = state.config.feedback.clone();
    Ok(state
        .db
        .call(move |c| feedback::settings(c, &config))
        .await?)
}

fn off() -> Error {
    Error::new(
        StatusCode::NOT_FOUND,
        error_code::FEEDBACK_OFF,
        "this server does not take feedback",
    )
}

fn rate_limited(seconds: u64) -> Error {
    Error::new(
        StatusCode::TOO_MANY_REQUESTS,
        error_code::RATE_LIMITED,
        "too many reports, try again later",
    )
    .retry_after(seconds)
}

/// `GET /api/v1/feedback/info`
pub async fn info(State(state): State<AppState>) -> Result<Json<FeedbackInfo>, Error> {
    Ok(Json(current(&state).await?.info()))
}

/// `POST /api/v1/feedback/answers`: how a sender's own reports stand, by
/// the secrets only their app keeps. Rate limited as a challenge is.
pub async fn answers(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(request): Body<app_protocol::feedback::AnswersRequest>,
) -> Result<Json<app_protocol::feedback::Answers>, Error> {
    let now = state.clock.now();
    state
        .feedback_limit
        .check(ip.0, now)
        .map_err(rate_limited)?;
    if request.secrets.len() > app_protocol::feedback::MAX_ASKED
        || request
            .secrets
            .iter()
            .any(|s| s.is_empty() || s.len() > 128)
    {
        return Err(Error::invalid("too many secrets, or not secrets"));
    }
    let reports = state
        .feedback
        .call(move |c| feedback::answers(c, &request.secrets))
        .await?;
    Ok(Json(app_protocol::feedback::Answers { reports }))
}

/// `POST /api/v1/feedback/challenge`
pub async fn challenge(
    State(state): State<AppState>,
    ip: ClientIp,
) -> Result<Json<Challenge>, Error> {
    if !current(&state).await?.receive {
        return Err(off());
    }
    let now = state.clock.now();
    state
        .feedback_limit
        .check(ip.0, now)
        .map_err(rate_limited)?;
    let config = state.config.feedback.clone();
    let challenge = state
        .feedback
        .call(move |c| {
            let address = feedback::address_key(c, ip.0)?;
            feedback::challenge(c, now, &address, &config)
        })
        .await?;
    Ok(Json(challenge))
}

/// `POST /api/v1/feedback`: answers 204 whether the report was kept, filed
/// as spam or dropped, so a sender learns nothing of the filters.
pub async fn send(State(state): State<AppState>, ip: ClientIp, req: Request) -> Response {
    match take(&state, ip, req).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => err.into_response(),
    }
}

async fn take(state: &AppState, ip: ClientIp, req: Request) -> Result<(), Error> {
    if !current(state).await?.receive {
        return Err(off());
    }
    // this instance's own people are not capped: an app signed in here
    // sends its token along. The report is never tied to the account.
    let bearer = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| token::hash(t.trim()));
    let member = match bearer {
        Some(hash) => {
            let now = state.clock.now();
            state
                .db
                .call(move |c| store::authenticate(c, &hash, now))
                .await?
                .is_some()
        }
        None => false,
    };
    // bodies are read before their proof of work can be checked: a few at
    // a time, and a body said to be too large is not read at all
    let declared = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    if declared.is_some_and(|n| n > MAX_REQUEST) {
        return Err(Error::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            error_code::INVALID,
            "this report is too large",
        ));
    }
    let busy = || {
        Error::new(
            StatusCode::SERVICE_UNAVAILABLE,
            error_code::RATE_LIMITED,
            "busy, send again in a moment",
        )
        .retry_after(30)
    };
    let _reading = tokio::time::timeout(READ_TIME, state.feedback_reading.acquire())
        .await
        .map_err(|_| busy())?
        .map_err(|_| busy())?;
    let body = tokio::time::timeout(
        READ_TIME,
        axum::body::to_bytes(req.into_body(), MAX_REQUEST),
    )
    .await
    .map_err(|_| Error::new(StatusCode::REQUEST_TIMEOUT, error_code::INVALID, "too slow"))?
    .map_err(|_| {
        Error::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            error_code::INVALID,
            "the report is too large",
        )
    })?;
    let report: Report =
        serde_json::from_slice(&body).map_err(|e| Error::invalid(format!("not a report: {e}")))?;
    let (challenge, nonce) = (report.challenge.clone(), report.nonce);
    let incoming = feedback::validate(report).map_err(Error::invalid)?;
    let now = state.clock.now();
    // the admin page's caps, over the config's
    let caps = state.db.call(|c| feedback::caps(c)).await?;
    let config = caps.over(&state.config.feedback);
    let outcome = state
        .feedback
        .call(move |c| {
            // the work is checked before anything else counts, a limit is
            // never found out for free
            if let Err(refused) = feedback::take_challenge(c, now, &challenge, nonce)? {
                return Ok(Err(Error::new(
                    StatusCode::BAD_REQUEST,
                    error_code::CHALLENGE_INVALID,
                    refused.as_str(),
                )));
            }
            let address = feedback::address_key(c, ip.0)?;
            let sender = feedback::Sender {
                address: &address,
                install: incoming.install.as_deref(),
                server: incoming.server.as_deref(),
                member,
            };
            if let Err(wait) = feedback::check_sender(c, now, sender, &config)? {
                return Ok(Err(rate_limited(wait)));
            }
            Ok(Ok(feedback::take(c, now, &address, &incoming)?))
        })
        .await??;
    match outcome {
        Outcome::Saved(id) => tracing::info!("feedback report {id}"),
        Outcome::Spam(id) => tracing::info!("feedback report {id} filed as spam"),
        Outcome::Dropped => tracing::info!("feedback report from a blocked sender dropped"),
    }
    Ok(())
}

/// `GET /api/v1/admin/feedback/settings`
pub async fn settings(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<SettingsView>, Error> {
    admin(&me)?;
    Ok(Json(settings_view(&state).await?))
}

/// `PUT /api/v1/admin/feedback/settings`
pub async fn set_settings(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(settings): Body<Settings>,
) -> Result<Json<SettingsView>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    state
        .db
        .call(move |c| {
            feedback::set_settings(c, settings)?;
            admin_event(
                c,
                now,
                "feedback_settings",
                None,
                &me,
                &ip_text,
                &serde_json::to_string(&settings).unwrap_or_default(),
            )
        })
        .await?;
    Ok(Json(settings_view(&state).await?))
}

/// `PUT /api/v1/admin/feedback/caps`: the caps against floods, each over
/// the config's, an empty one back to it.
pub async fn set_caps(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(caps): Body<Caps>,
) -> Result<Json<SettingsView>, Error> {
    admin(&me)?;
    if !caps.valid() {
        return Err(Error::invalid(format!(
            "a cap is between 1 and {}",
            feedback::MAX_CAP
        )));
    }
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    state
        .db
        .call(move |c| {
            feedback::set_caps(c, caps)?;
            admin_event(
                c,
                now,
                "feedback_caps",
                None,
                &me,
                &ip_text,
                &serde_json::to_string(&caps).unwrap_or_default(),
            )
        })
        .await?;
    Ok(Json(settings_view(&state).await?))
}

/// Who is reading the inbox.
struct Moderator {
    /// The person, `None` for the read-only token of `reports pull`.
    me: Option<Authed>,
}

impl Moderator {
    /// Marks, blocks and deletes need a person, the token only reads.
    fn writer(&self) -> Result<&Authed, Error> {
        self.me.as_ref().ok_or_else(Error::forbidden)
    }
}

/// The read-only token's stored form, in settings.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenRow {
    hash: String,
    created_at: i64,
}

fn read_token(c: &Connection) -> Result<Option<TokenRow>, DbError> {
    let text: Option<String> = c
        .query_row(
            "SELECT value FROM settings WHERE key = 'feedback_token'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
}

fn is_reader(c: &Connection, user_id: &str) -> Result<bool, DbError> {
    Ok(c.query_row(
        "SELECT count(*) FROM feedback_readers WHERE user_id = ?1",
        [user_id],
        |r| r.get::<_, i64>(0),
    )? > 0)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// An admin, a reader, or the read-only token.
async fn moderator(state: &AppState, headers: &HeaderMap) -> Result<Moderator, Error> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .ok_or_else(Error::unauthorized)?;
    let hash = token::hash(bearer);
    let now = state.clock.now();
    state
        .db
        .call(move |c| {
            if read_token(c)?.is_some_and(|t| t.hash == hex(&hash)) {
                return Ok(Ok(Moderator { me: None }));
            }
            let Some(me) = store::authenticate(c, &hash, now)? else {
                return Ok(Err(Error::unauthorized()));
            };
            if me.user.acts_as_admin() || is_reader(c, &me.user.id)? {
                Ok(Ok(Moderator { me: Some(me) }))
            } else {
                Ok(Err(Error::forbidden()))
            }
        })
        .await?
}

fn no_report() -> Error {
    Error::new(
        StatusCode::NOT_FOUND,
        error_code::NOT_FOUND,
        "no such report",
    )
}

/// Logs a moderator's action.
async fn moderated(state: &AppState, me: &Authed, ip: ClientIp, name: &'static str, more: String) {
    let now = state.clock.now();
    let me = me.clone();
    let ip = ip.0.to_string();
    let logged = state
        .db
        .call(move |c| admin_event(c, now, name, None, &me, &ip, &more))
        .await;
    if let Err(err) = logged {
        tracing::warn!("security log: {err}");
    }
}

/// `GET /api/v1/admin/feedback/access`: what the signed-in person may do
/// with feedback, so the page shows the inbox to readers too.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Access {
    inbox: bool,
    admin: bool,
}

/// `GET /api/v1/admin/feedback/access`
pub async fn access(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<Access>, Error> {
    let admin = me.user.acts_as_admin();
    let id = me.user.id.clone();
    let reader = state.db.call(move |c| is_reader(c, &id)).await?;
    Ok(Json(Access {
        inbox: admin || reader,
        admin,
    }))
}

/// `GET /api/v1/admin/feedback/reports`
pub async fn reports(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(filter): Query<inbox::Filter>,
) -> Result<Json<inbox::Page>, Error> {
    moderator(&state, &headers).await?;
    Ok(Json(
        state
            .feedback
            .call(move |c| inbox::list(c, &filter))
            .await?,
    ))
}

/// `GET /api/v1/admin/feedback/reports/{id}`
pub async fn report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<inbox::Full>, Error> {
    moderator(&state, &headers).await?;
    state
        .feedback
        .call(move |c| inbox::get(c, &id))
        .await?
        .map(Json)
        .ok_or_else(no_report)
}

/// `GET /api/v1/admin/feedback/reports/{id}/images/{n}`: a download, never
/// shown by the browser on its own.
pub async fn image(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, position)): Path<(String, i64)>,
) -> Result<Response, Error> {
    moderator(&state, &headers).await?;
    let name = format!("{id}-{position}");
    let (kind, bytes) = state
        .feedback
        .call(move |c| inbox::image(c, &id, position))
        .await?
        .ok_or_else(no_report)?;
    let mime = match kind.as_str() {
        "png" => "image/png",
        "jpeg" => "image/jpeg",
        _ => "image/webp",
    };
    Ok((
        [
            (header::CONTENT_TYPE, mime.to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}.{kind}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// `PATCH /api/v1/admin/feedback/reports/{id}`
pub async fn change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Body(change): Body<inbox::Change>,
) -> Result<Json<inbox::Full>, Error> {
    moderator(&state, &headers).await?.writer()?;
    if change
        .note
        .as_ref()
        .is_some_and(|n| n.chars().count() > 2_000)
    {
        return Err(Error::invalid("the note is over 2000 characters"));
    }
    if change
        .answer
        .as_ref()
        .is_some_and(|n| n.chars().count() > 2_000)
    {
        return Err(Error::invalid("the answer is over 2000 characters"));
    }
    state
        .feedback
        .call(move |c| {
            if inbox::update(c, &id, &change)? {
                inbox::get(c, &id)
            } else {
                Ok(None)
            }
        })
        .await?
        .map(Json)
        .ok_or_else(no_report)
}

/// `DELETE /api/v1/admin/feedback/reports/{id}`
pub async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    ip: ClientIp,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    let moderator = moderator(&state, &headers).await?;
    let me = moderator.writer()?;
    let gone = id.clone();
    if !state
        .feedback
        .call(move |c| inbox::delete(c, &gone))
        .await?
    {
        return Err(no_report());
    }
    moderated(&state, me, ip, "feedback_deleted", id).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/admin/feedback/reports/{id}/block`
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockSender {
    kind: BlockKind,
    /// Days the block lasts, for ever without.
    #[serde(default)]
    days: Option<u32>,
}

/// `POST /api/v1/admin/feedback/reports/{id}/block`
pub async fn block_sender(
    State(state): State<AppState>,
    headers: HeaderMap,
    ip: ClientIp,
    Path(id): Path<String>,
    Body(body): Body<BlockSender>,
) -> Result<Json<inbox::Full>, Error> {
    let moderator = moderator(&state, &headers).await?;
    let me = moderator.writer()?;
    let now = state.clock.now();
    let report = id.clone();
    let kind = body.kind;
    let (blocked, full) = state
        .feedback
        .call(move |c| {
            let blocked = inbox::block_sender(c, now, &report, kind, body.days)?;
            Ok((blocked, inbox::get(c, &report)?))
        })
        .await?;
    match blocked {
        None => return Err(no_report()),
        Some(false) => return Err(Error::invalid("the report has nothing of that kind")),
        Some(true) => {}
    }
    moderated(
        &state,
        me,
        ip,
        "feedback_blocked",
        format!("{} from report {id}", kind.as_str()),
    )
    .await;
    full.map(Json).ok_or_else(no_report)
}

/// `GET /api/v1/admin/feedback/blocks`
pub async fn blocks(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<inbox::Block>>, Error> {
    moderator(&state, &headers).await?.writer()?;
    let now = state.clock.now();
    Ok(Json(
        state.feedback.call(move |c| inbox::blocks(c, now)).await?,
    ))
}

/// `POST /api/v1/admin/feedback/blocks`: a word, an email or an install
/// typed in.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewBlock {
    kind: BlockKind,
    value: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    days: Option<u32>,
}

/// `POST /api/v1/admin/feedback/blocks`
pub async fn add_block(
    State(state): State<AppState>,
    headers: HeaderMap,
    ip: ClientIp,
    Body(new): Body<NewBlock>,
) -> Result<Json<Vec<inbox::Block>>, Error> {
    let moderator = moderator(&state, &headers).await?;
    let me = moderator.writer()?;
    let value = new.value.trim().to_owned();
    if value.is_empty() || value.chars().count() > 254 || new.note.chars().count() > 500 {
        return Err(Error::invalid(
            "a block needs a value of 254 characters at most",
        ));
    }
    let now = state.clock.now();
    let kind = new.kind;
    let list = state
        .feedback
        .call(move |c| {
            feedback::block(
                c,
                now,
                kind,
                &value,
                &new.note,
                new.days.map(|d| now + i64::from(d) * DAY),
            )?;
            inbox::blocks(c, now)
        })
        .await?;
    moderated(&state, me, ip, "feedback_blocked", kind.as_str().to_owned()).await;
    Ok(Json(list))
}

/// `DELETE /api/v1/admin/feedback/blocks?kind=&value=`
#[derive(Deserialize)]
pub struct Unblock {
    kind: BlockKind,
    value: String,
}

/// `DELETE /api/v1/admin/feedback/blocks`
pub async fn remove_block(
    State(state): State<AppState>,
    headers: HeaderMap,
    ip: ClientIp,
    Query(gone): Query<Unblock>,
) -> Result<Json<Vec<inbox::Block>>, Error> {
    let moderator = moderator(&state, &headers).await?;
    let me = moderator.writer()?;
    let now = state.clock.now();
    let kind = gone.kind;
    let list = state
        .feedback
        .call(move |c| {
            inbox::unblock(c, kind, &gone.value)?;
            inbox::blocks(c, now)
        })
        .await?;
    moderated(
        &state,
        me,
        ip,
        "feedback_unblocked",
        kind.as_str().to_owned(),
    )
    .await;
    Ok(Json(list))
}

/// A reader of the inbox.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reader {
    id: String,
    username: String,
    added_at: i64,
}

/// `GET /api/v1/admin/feedback/settings`: the switches, the readers and
/// whether a pull token exists.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    #[serde(flatten)]
    settings: Settings,
    readers: Vec<Reader>,
    /// When the read-only token was made.
    token_created_at: Option<i64>,
    /// The caps set on the page, empty for the config's.
    caps: Caps,
    /// The config's caps, what an empty one means.
    default_caps: Caps,
}

async fn settings_view(state: &AppState) -> Result<SettingsView, Error> {
    let settings = current(state).await?;
    let (readers, token) = state
        .db
        .call(|c| {
            let mut stmt = c.prepare(
                "SELECT u.id, u.username, r.added_at FROM feedback_readers r
                 JOIN users u ON u.id = r.user_id ORDER BY u.username",
            )?;
            let readers = stmt
                .query_map([], |r| {
                    Ok(Reader {
                        id: r.get(0)?,
                        username: r.get(1)?,
                        added_at: r.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok((readers, read_token(c)?.map(|t| t.created_at)))
        })
        .await?;
    let caps = state.db.call(|c| feedback::caps(c)).await?;
    Ok(SettingsView {
        settings,
        readers,
        token_created_at: token,
        caps,
        default_caps: Caps::of(&state.config.feedback),
    })
}

/// `PUT /api/v1/admin/feedback/readers/{username}`
pub async fn add_reader(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(username): Path<String>,
) -> Result<Json<SettingsView>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip = ip.0.to_string();
    let found = state
        .db
        .call(move |c| {
            let Some(user) = store::user_by_name(c, &username)? else {
                return Ok(false);
            };
            c.execute(
                "INSERT OR IGNORE INTO feedback_readers (user_id, added_at) VALUES (?1, ?2)",
                rusqlite::params![user.id, now],
            )?;
            admin_event(
                c,
                now,
                "feedback_reader_added",
                Some(&user.id),
                &me,
                &ip,
                "",
            )?;
            Ok(true)
        })
        .await?;
    if !found {
        return Err(Error::new(
            StatusCode::NOT_FOUND,
            error_code::NOT_FOUND,
            "no such person",
        ));
    }
    Ok(Json(settings_view(&state).await?))
}

/// `DELETE /api/v1/admin/feedback/readers/{id}`
pub async fn remove_reader(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<SettingsView>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip = ip.0.to_string();
    state
        .db
        .call(move |c| {
            if c.execute("DELETE FROM feedback_readers WHERE user_id = ?1", [&id])? > 0 {
                admin_event(c, now, "feedback_reader_removed", Some(&id), &me, &ip, "")?;
            }
            Ok(())
        })
        .await?;
    Ok(Json(settings_view(&state).await?))
}

/// `POST /api/v1/admin/feedback/token`: a new read-only token for
/// `reports pull`, shown once, the old one stops working.
#[derive(Serialize)]
pub struct NewToken {
    token: String,
}

/// `POST /api/v1/admin/feedback/token`
pub async fn new_token(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
) -> Result<Json<NewToken>, Error> {
    admin(&me)?;
    let (secret, hash) = token::new_token(token::Kind::Feedback);
    let now = state.clock.now();
    let ip = ip.0.to_string();
    let row = serde_json::to_string(&TokenRow {
        hash: hex(&hash),
        created_at: now,
    })
    .unwrap_or_default();
    state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO settings (key, value) VALUES ('feedback_token', ?1)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                [row],
            )?;
            admin_event(c, now, "feedback_token", None, &me, &ip, "made")
        })
        .await?;
    Ok(Json(NewToken { token: secret }))
}

/// `DELETE /api/v1/admin/feedback/token`
pub async fn revoke_token(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
) -> Result<StatusCode, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip = ip.0.to_string();
    state
        .db
        .call(move |c| {
            if c.execute("DELETE FROM settings WHERE key = 'feedback_token'", [])? > 0 {
                admin_event(c, now, "feedback_token", None, &me, &ip, "revoked")?;
            }
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
