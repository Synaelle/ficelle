// SPDX-License-Identifier: AGPL-3.0-or-later

//! Attachment files over http: the policy and usage of an account, the
//! state of a file, uploads in parts, downloads with ranges, release.
//! `space=` names a shared space as for sync: members read, editors and the
//! owner write. See [`crate::files`] for how files are kept.

use std::io::Read;
use std::pin::Pin;
use std::task::{Context, Poll};

use app_e2ee::CHUNK;
use app_protocol::e2ee::AccountKind;
use app_protocol::error_code;
use app_protocol::files::{
    FileState, FilesInfo, FilesPolicy, MAX_PART, NAME_HEADER, TYPE_HEADER, decode_name,
};
use app_protocol::sync::SpaceRole;
use axum::Json;
use axum::body::{Body as HttpBody, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::extract::CurrentUser;
use super::sync::{Target, target};
use crate::error::Error;
use crate::files::{self as store, Incoming, Part, PlainReader, Refusal, policy};
use crate::routes::AppState;

/// Bytes handed to the network at a time when reading a file.
const READ_PIECE: usize = 256 * 1024;

/// `space=` and, to release a file, `head=`.
#[derive(Debug, Default, Deserialize)]
pub struct Where {
    #[serde(default)]
    space: Option<String>,
    #[serde(default)]
    head: Option<i64>,
}

fn file_error(status: StatusCode, code: &'static str, why: impl Into<String>) -> Error {
    Error::new(status, code, why)
}

fn invalid(why: &str) -> Error {
    file_error(StatusCode::BAD_REQUEST, error_code::FILE_INVALID, why)
}

fn checked(address: &str) -> Result<(), Error> {
    if store::address_ok(address) {
        Ok(())
    } else {
        Err(invalid("a file address is 64 lower case hex characters"))
    }
}

fn writer(target: &Target) -> Result<(), Error> {
    if target.role == SpaceRole::Read {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            error_code::FORBIDDEN,
            "this is shared with you to read only",
        ));
    }
    Ok(())
}

/// The largest part this server takes, whole chunks under the body limit.
pub(crate) fn part_bytes(state: &AppState) -> u64 {
    let body = u64::from(state.config.server.max_body_mb) * 1024 * 1024;
    let chunk = CHUNK as u64;
    (MAX_PART.min(body.saturating_sub(chunk)) / chunk).max(1) * chunk
}

async fn policy_of(state: &AppState, owner: String) -> Result<FilesPolicy, Error> {
    let config = state.config.clone();
    Ok(state
        .db
        .call(move |c| policy::effective(c, &config.files, &owner))
        .await?)
}

async fn owner(state: &AppState, space: String) -> Result<String, Error> {
    Ok(state
        .db
        .call(move |c| store::owner_of(c, &space))
        .await?
        .unwrap_or_default())
}

/// `GET /api/v1/sync/files/policy`
pub async fn info(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<FilesInfo>, Error> {
    let policy = policy_of(&state, me.user.id.clone()).await?;
    let id = me.user.id.clone();
    let usage = state.db.call(move |c| store::usage(c, &id)).await?;
    Ok(Json(FilesInfo {
        policy,
        usage,
        part_bytes: part_bytes(&state),
    }))
}

/// `GET /api/v1/sync/files/{address}/state?space=`
pub async fn state_of(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(address): Path<String>,
    Query(at): Query<Where>,
) -> Result<Json<FileState>, Error> {
    checked(&address)?;
    let target = target(&state, &me, at.space).await?;
    let space = target.space.id;
    Ok(Json(
        state
            .db
            .call(move |c| store::state(c, &space, &address))
            .await?,
    ))
}

/// `POST /api/v1/sync/files/{address}/keep?space=`: takes back a file the
/// space released, while it is kept.
pub async fn keep(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(address): Path<String>,
    Query(at): Query<Where>,
) -> Result<StatusCode, Error> {
    checked(&address)?;
    let target = target(&state, &me, at.space).await?;
    writer(&target)?;
    let space = target.space.id;
    let config = state.config.files.clone();
    match state
        .db
        .call(move |c| store::keep(c, &config, &space, &address))
        .await?
    {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err(Error::not_found()),
        Err(refusal) => Err(refused(&refusal)),
    }
}

/// `Content-Range: bytes 0-65535/200000`, or `bytes */0` for an empty file.
fn content_range(headers: &HeaderMap) -> Option<Incoming> {
    let text = headers.get(header::CONTENT_RANGE)?.to_str().ok()?;
    let rest = text.strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let total: u64 = total.parse().ok()?;
    if range == "*" {
        return (total == 0).then_some(Incoming {
            start: 0,
            len: 0,
            total: 0,
        });
    }
    let (start, end) = range.split_once('-')?;
    let (start, end): (u64, u64) = (start.parse().ok()?, end.parse().ok()?);
    (end >= start && end < total).then_some(Incoming {
        start,
        len: end - start + 1,
        total,
    })
}

/// The name and type an upload declares, and what its first bytes say.
fn type_allowed(policy: &FilesPolicy, headers: &HeaderMap, first: &[u8]) -> bool {
    let read = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let name = decode_name(&read(NAME_HEADER)).unwrap_or_default();
    let mime = read(TYPE_HEADER);
    if !policy::allows(policy, &name, &mime) {
        return false;
    }
    match policy::sniff(first) {
        Some((ext, mime)) => policy::allows(policy, &format!("file{ext}"), mime),
        None => true,
    }
}

fn refused(refusal: &Refusal) -> Error {
    match refusal {
        Refusal::TooLarge => file_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            error_code::FILE_TOO_LARGE,
            "this file is larger than allowed",
        ),
        Refusal::QuotaFull => file_error(
            StatusCode::INSUFFICIENT_STORAGE,
            error_code::QUOTA_FULL,
            "the storage of this space's owner is full",
        ),
        Refusal::Misplaced(received) => file_error(
            StatusCode::CONFLICT,
            error_code::UPLOAD_MOVED,
            format!("this upload stands at {received} bytes"),
        ),
        Refusal::Malformed => invalid("parts are whole 64 KB chunks, of a size a file can have"),
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Error::internal(&e.to_string()))?
        .map_err(|e| Error::internal(&format!("file storage: {e}")))
}

/// `PUT /api/v1/sync/files/{address}?space=`, one part with a
/// `Content-Range`. Answers where the file stands after it.
pub async fn put(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(address): Path<String>,
    Query(at): Query<Where>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<FileState>, Error> {
    checked(&address)?;
    let part = content_range(&headers).ok_or_else(|| invalid("a part needs a Content-Range"))?;
    if part.len != body.len() as u64 || part.len > part_bytes(&state) {
        return Err(invalid("the part does not match its Content-Range"));
    }
    let target = target(&state, &me, at.space).await?;
    writer(&target)?;
    let kind = target.space.kind;
    let space = target.space.id.clone();
    let policy = policy_of(&state, owner(&state, space.clone()).await?).await?;
    // standard files show what they are: the type rules apply here too
    if kind == AccountKind::Standard && part.start == 0 && !type_allowed(&policy, &headers, &body) {
        return Err(file_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            error_code::FILE_TYPE_REFUSED,
            "this type of file is not allowed",
        ));
    }
    let now = state.clock.now();
    // from taking the part to finishing the file: no part lands while a
    // finished upload is checked and moved
    let _held = state.files_lock.lock().await;
    let (s, a) = (space.clone(), address.clone());
    let plan = state
        .db
        .call(move |c| store::begin_part(c, kind, &s, &a, part, &policy, now))
        .await?
        .map_err(|r| refused(&r))?;
    let Part::Write { at: offset } = plan else {
        return state_answer(&state, space, address).await;
    };
    let upload = store::upload_name(&space, &address);
    let (files, master, a) = (state.files.clone(), state.master.clone(), address.clone());
    let up = upload.clone();
    blocking(move || {
        let stored = store::stored_part(kind, &master, &a, part, &body)
            .map_err(|_| std::io::Error::other("part does not seal"))?;
        files.write_incoming(&up, offset, &stored)
    })
    .await?;
    let (s, a) = (space.clone(), address.clone());
    let complete = state
        .db
        .call(move |c| store::part_written(c, &s, &a, part, now))
        .await?;
    if complete {
        finish(&state, kind, &space, &address, part.total).await?;
    }
    state_answer(&state, space, address).await
}

async fn state_answer(
    state: &AppState,
    space: String,
    address: String,
) -> Result<Json<FileState>, Error> {
    Ok(Json(
        state
            .db
            .call(move |c| store::state(c, &space, &address))
            .await?,
    ))
}

/// A complete upload: checked (standard: the content is the one its
/// address names), recorded, moved into place. The caller holds
/// `files_lock`.
async fn finish(
    state: &AppState,
    kind: AccountKind,
    space: &str,
    address: &str,
    total: u64,
) -> Result<(), Error> {
    let upload = store::upload_name(space, address);
    // two devices sending the same file (the same bytes) may both get here
    let (s, a) = (space.to_owned(), address.to_owned());
    let now_state = state.db.call(move |c| store::state(c, &s, &a)).await?;
    if now_state.status == app_protocol::files::FileStatus::Complete {
        return Ok(());
    }
    if kind == AccountKind::Standard {
        let (files, master, a, up) = (
            state.files.clone(),
            state.master.clone(),
            address.to_owned(),
            upload.clone(),
        );
        let good = blocking(move || {
            let mut source = files.read_incoming(&up)?;
            store::check_standard(&mut source, &master, &a, total)
        })
        .await?;
        if !good {
            let (s, a) = (space.to_owned(), address.to_owned());
            state.db.call(move |c| store::abandon(c, &s, &a)).await?;
            let (files, up) = (state.files.clone(), upload.clone());
            blocking(move || files.discard(&up)).await?;
            return Err(invalid("the content does not match its address"));
        }
    }
    let now = state.clock.now();
    let (s, a) = (space.to_owned(), address.to_owned());
    let object = state
        .db
        .call(move |c| store::completed(c, kind, &s, &a, total, now))
        .await?;
    let files = state.files.clone();
    blocking(move || files.complete(&upload, &object)).await
}

/// `Range: bytes=N-` or `bytes=N-M`, within a file of `size` bytes.
fn range(headers: &HeaderMap, size: u64) -> Option<(u64, u64)> {
    let text = headers.get(header::RANGE)?.to_str().ok()?;
    let (start, end) = text.strip_prefix("bytes=")?.split_once('-')?;
    let start: u64 = start.parse().ok()?;
    let end: u64 = if end.is_empty() {
        size.checked_sub(1)?
    } else {
        end.parse::<u64>().ok()?.min(size.checked_sub(1)?)
    };
    (start <= end).then_some((start, end))
}

/// The pieces a blocking reader sends to the response.
struct Pieces(tokio::sync::mpsc::Receiver<std::io::Result<Bytes>>);

impl futures_core::Stream for Pieces {
    type Item = std::io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// `GET /api/v1/sync/files/{address}?space=`, the content, from a `Range`
/// when one is given.
pub async fn get(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(address): Path<String>,
    Query(at): Query<Where>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    checked(&address)?;
    let target = target(&state, &me, at.space).await?;
    let kind = target.space.kind;
    let space = target.space.id;
    let a = address.clone();
    let (object, size) = state
        .db
        .call(move |c| store::readable(c, &space, &a))
        .await?
        .ok_or_else(Error::not_found)?;
    let wanted = range(&headers, size);
    if headers.contains_key(header::RANGE) && wanted.is_none() && size > 0 {
        let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes */{size}"))
                .map_err(|e| Error::internal(&e.to_string()))?,
        );
        return Ok(response);
    }
    let (start, end) = wanted.unwrap_or((0, size.saturating_sub(1)));
    let len = if size == 0 { 0 } else { end - start + 1 };
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let (files, master) = (state.files.clone(), state.master.clone());
    tokio::task::spawn_blocking(move || {
        let opened: std::io::Result<Box<dyn Read + Send>> =
            match kind {
                AccountKind::Standard => files
                    .read(&object, PlainReader::stored_offset(start))
                    .map(|source| {
                        Box::new(PlainReader::new(source, &master, &address, size, start))
                            as Box<dyn Read + Send>
                    }),
                AccountKind::E2ee => files.read(&object, start),
            };
        let mut reader = match opened {
            Ok(reader) => reader.take(len),
            Err(e) => {
                let _ = tx.blocking_send(Err(e));
                return;
            }
        };
        let mut buf = vec![0u8; READ_PIECE];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx
                        .blocking_send(Ok(Bytes::copy_from_slice(&buf[..n])))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.blocking_send(Err(e));
                    break;
                }
            }
        }
    });
    let mut response = HttpBody::from_stream(Pieces(rx)).into_response();
    let out = response.headers_mut();
    let value =
        |text: String| HeaderValue::from_str(&text).map_err(|e| Error::internal(&e.to_string()));
    out.insert(header::CONTENT_LENGTH, value(len.to_string())?);
    out.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    out.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if wanted.is_some() {
        out.insert(
            header::CONTENT_RANGE,
            value(format!("bytes {start}-{end}/{size}"))?,
        );
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
    }
    Ok(response)
}

/// `DELETE /api/v1/sync/files/{address}?space=&head=`: the space no longer
/// uses the file, if nothing was pushed to it since `head`.
pub async fn release(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path(address): Path<String>,
    Query(at): Query<Where>,
) -> Result<StatusCode, Error> {
    checked(&address)?;
    let head = at
        .head
        .ok_or_else(|| invalid("a release names the head it saw"))?;
    let target = target(&state, &me, at.space).await?;
    writer(&target)?;
    let space = target.space.id;
    let now = state.clock.now();
    if state
        .db
        .call(move |c| store::release(c, &space, &address, head, now))
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(file_error(
            StatusCode::CONFLICT,
            error_code::SPACE_MOVED_ON,
            "the space changed since, sync and try again",
        ))
    }
}

/// Forgets what retention allows, deletes the objects and uploads nothing
/// uses, and leftovers of uploads the database no longer knows. Returns how
/// many objects went.
///
/// # Errors
///
/// Database or storage errors.
pub async fn sweep(state: &AppState) -> Result<usize, Error> {
    let _held = state.files_lock.lock().await;
    let now = state.clock.now();
    let config = state.config.clone();
    let (objects, uploads, live) = state
        .db
        .call(move |c| {
            let retention = |c: &rusqlite::Connection, owner: &str| {
                Ok(policy::effective(c, &config.files, owner)?.retention_days)
            };
            let (objects, uploads) = store::sweep(c, now, &retention)?;
            Ok((objects, uploads, store::live_uploads(c)?))
        })
        .await?;
    let gone = objects.len();
    let files = state.files.clone();
    blocking(move || {
        for object in &objects {
            files.delete(object)?;
        }
        for upload in &uploads {
            files.discard(upload)?;
        }
        for upload in files.incoming()? {
            if !live.contains(&upload) {
                files.discard(&upload)?;
            }
        }
        Ok(())
    })
    .await?;
    Ok(gone)
}

/// Where a file stands, for tests.
#[cfg(test)]
pub(super) fn status_of(value: &serde_json::Value) -> app_protocol::files::FileStatus {
    serde_json::from_value(value["status"].clone()).unwrap()
}
