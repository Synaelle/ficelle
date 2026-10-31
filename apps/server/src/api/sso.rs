// SPDX-License-Identifier: AGPL-3.0-or-later

//! Single sign-on. The page asks for the provider's address, the browser
//! goes there and comes back to [`callback`], which sends it on to the page
//! with a one minute code in the fragment. The page trades that code for a
//! session like any other sign-in. Fragments never reach servers or logs,
//! and the session itself never appears in an address.
//!
//! A provider sign-in needs no second factor here: the provider has its own.

use app_protocol::account::Role;
use app_protocol::e2ee::{AccountKind, AccountKinds};
use app_protocol::error_code;
use app_protocol::security::{Confirm, SsoExchangeRequest, SsoIdentity, SsoStart, SsoStartRequest};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderName;
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::response::{AppendHeaders, IntoResponse, Redirect, Response};
use serde::{Deserialize, Serialize};

use super::auth::{finish_sign_in, not_disabled, rate_limit};
use super::extract::{Body, ClientIp, CurrentUser};
use super::security::{confirm, log};
use crate::accounts::factors::{self, ChallengeKind};
use crate::accounts::store::{self, Event};
use crate::accounts::{device_info, display_name, e2ee, sso_username, token};
use crate::clock::MINUTE;
use crate::error::Error;
use crate::routes::AppState;
use crate::sso::{Identity, Provider};

/// How long the provider's page may take.
const START_LIFE: i64 = 10 * MINUTE;
/// How long the page has to trade the code.
const GRANT_LIFE: i64 = MINUTE;

/// Kept while the browser is at the provider.
#[derive(Debug, Serialize, Deserialize)]
struct Started {
    verifier: String,
    nonce: String,
    /// Linking an identity to the signed in account, not signing in.
    link: bool,
    return_to: String,
    /// Hash of the cookie given to the browser that started: the provider's
    /// answer counts only in that browser, so nobody can send someone their
    /// own provider link (to sign them in as the sender, or to link the
    /// sender's account to the other person's identity).
    #[serde(default)]
    binding: Vec<u8>,
}

/// The cookie tying a sign-on to the browser that started it.
const BINDING_COOKIE: &str = "sso_binding";

fn binding_cookie(state: &AppState, value: &str, max_age: i64) -> String {
    let secure = if state.config.hsts() { "; Secure" } else { "" };
    format!(
        "{BINDING_COOKIE}={value}; Path={}{}; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}",
        state.config.base_path(),
        crate::sso::CALLBACK_PATH,
    )
}

/// The same cookie for the code's exchange, from the callback on: the
/// code a page holds signs in only the browser the sign-in went through,
/// so nobody can send an admin a link with their own code.
fn exchange_cookie(state: &AppState, value: &str) -> String {
    let secure = if state.config.hsts() { "; Secure" } else { "" };
    format!(
        "{BINDING_COOKIE}={value}; Path={}/api/v1/auth/sso/exchange; Max-Age={}; HttpOnly; SameSite=Strict{secure}",
        state.config.base_path(),
        GRANT_LIFE / 1000,
    )
}

/// What a sign-in code holds: the identity's label for the log, and the
/// hash of the browser's binding cookie.
#[derive(Serialize, Deserialize)]
struct Granted {
    label: String,
    binding: Vec<u8>,
}

/// The binding cookie the browser sent back, if any.
fn sent_binding(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == BINDING_COOKIE)
        .map(|(_, value)| value.to_owned())
}

/// Equal hashes, in a time that does not depend on where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn provider(state: &AppState) -> Result<std::sync::Arc<Provider>, Error> {
    state.sso.clone().ok_or_else(|| {
        Error::new(
            StatusCode::NOT_FOUND,
            error_code::SSO_UNAVAILABLE,
            "single sign-on is not set up on this server",
        )
    })
}

fn failed() -> Error {
    Error::new(
        StatusCode::UNAUTHORIZED,
        error_code::SSO_FAILED,
        "single sign-on did not work, or took too long",
    )
}

/// A path on this server: one slash, no scheme, no host, so the provider's
/// answer can only lead back here.
fn local_path(raw: &str) -> Result<String, Error> {
    let ok = raw.starts_with('/')
        && !raw.starts_with("//")
        && raw.len() <= 200
        && !raw.contains(['\\', '#'])
        && !raw.chars().any(char::is_control);
    if ok {
        Ok(raw.to_owned())
    } else {
        Err(Error::invalid("returnTo must be a path on this server"))
    }
}

async fn start(
    state: &AppState,
    user_id: Option<String>,
    return_to: &str,
) -> Result<(AppendHeaders<[(HeaderName, String); 1]>, Json<SsoStart>), Error> {
    let provider = provider(state)?;
    let return_to = local_path(return_to)?;
    let (state_token, hash) = token::new_token(token::Kind::Challenge);
    let started = provider.start(&state_token).await.map_err(|e| {
        tracing::warn!("single sign-on: {e}");
        Error::new(
            StatusCode::BAD_GATEWAY,
            error_code::SSO_FAILED,
            "the sign-on provider cannot be reached",
        )
    })?;
    let expires_at = state.clock.now() + START_LIFE;
    let (binding, binding_hash) = token::new_token(token::Kind::Challenge);
    let data = Started {
        binding: binding_hash.to_vec(),
        verifier: started.verifier,
        nonce: started.nonce,
        link: user_id.is_some(),
        return_to,
    };
    state
        .db
        .call(move |c| {
            factors::store_challenge(
                c,
                &hash,
                ChallengeKind::SsoStart,
                user_id.as_deref(),
                &data,
                expires_at,
            )
        })
        .await?;
    Ok((
        AppendHeaders([(
            SET_COOKIE,
            binding_cookie(state, &binding, START_LIFE / 1000),
        )]),
        Json(SsoStart { url: started.url }),
    ))
}

/// `POST /api/v1/auth/sso/start`
pub async fn sign_in_start(
    State(state): State<AppState>,
    ip: ClientIp,
    Body(req): Body<SsoStartRequest>,
) -> Result<impl IntoResponse, Error> {
    rate_limit(&state, ip)?;
    start(&state, None, &req.return_to).await
}

/// `POST /api/v1/me/sso/link`: links a provider identity to this account.
pub async fn link_start(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(req): Body<SsoStartRequest>,
) -> Result<impl IntoResponse, Error> {
    provider(&state)?;
    let given = Confirm {
        password: req.password,
        proof: req.proof,
    };
    confirm(&state, &me, given, ip).await?;
    start(&state, Some(me.user.id.clone()), &req.return_to).await
}

/// What the provider sends back.
#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

/// `GET /sso/callback`, outside the api: the browser lands here from the
/// provider and goes on to the page with a code or an error in the fragment.
pub async fn callback(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: axum::http::HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> (AppendHeaders<[(HeaderName, String); 2]>, Redirect) {
    // the cookie is spent either way, and goes on to the exchange
    let binding = sent_binding(&headers).unwrap_or_default();
    let cookies = AppendHeaders([
        (SET_COOKIE, binding_cookie(&state, "", 0)),
        (SET_COOKIE, exchange_cookie(&state, &binding)),
    ]);
    (cookies, arrive_here(&state, ip, &headers, query).await)
}

async fn arrive_here(
    state: &AppState,
    ip: ClientIp,
    headers: &axum::http::HeaderMap,
    query: CallbackQuery,
) -> Redirect {
    let now = state.clock.now();
    let Some(hash) = query.state.as_deref().map(|s| token::hash(s.trim())) else {
        return Redirect::to(&format!(
            "{}/admin#sso-error={}",
            state.config.base_path(),
            error_code::SSO_FAILED
        ));
    };
    let started = state
        .db
        .call(move |c| {
            let found = factors::challenge::<Started>(c, &hash, ChallengeKind::SsoStart, now)?;
            // single use, whatever the outcome
            factors::end_challenge(c, &hash)?;
            Ok(found)
        })
        .await;
    let Ok(Some(started)) = started else {
        return Redirect::to(&format!(
            "{}/admin#sso-error={}",
            state.config.base_path(),
            error_code::SSO_FAILED
        ));
    };
    let from_here =
        sent_binding(headers).is_some_and(|b| same(&token::hash(&b), &started.data.binding));
    if !from_here {
        tracing::warn!("single sign-on answer in another browser than the one that started it");
        return Redirect::to(&format!(
            "{}/admin#sso-error={}",
            state.config.base_path(),
            error_code::SSO_FAILED
        ));
    }
    let return_to = started.data.return_to.clone();
    let fragment = match arrive(state, ip, query, started.user_id, started.data).await {
        Ok(fragment) => fragment,
        Err(code) => format!("sso-error={code}"),
    };
    Redirect::to(&format!("{return_to}#{fragment}"))
}

/// The provider's answer checked and acted on: the fragment for the page,
/// or an error code.
async fn arrive(
    state: &AppState,
    ip: ClientIp,
    query: CallbackQuery,
    user_id: Option<String>,
    started: Started,
) -> Result<String, &'static str> {
    let provider = state.sso.clone().ok_or(error_code::SSO_UNAVAILABLE)?;
    if let Some(error) = query.error {
        tracing::info!("single sign-on refused by the provider: {error}");
        return Err(error_code::SSO_FAILED);
    }
    let code = query.code.ok_or(error_code::SSO_FAILED)?;
    let identity = provider
        .finish(&code, &started.verifier, &started.nonce)
        .await
        .map_err(|e| {
            tracing::warn!("single sign-on: {e}");
            error_code::SSO_FAILED
        })?;
    let ip = ip.0.to_string();
    if started.link {
        let user_id = user_id.ok_or(error_code::SSO_FAILED)?;
        return link(state, &user_id, &identity, &ip).await;
    }
    let grant = grant(state, &identity, &ip, started.binding)
        .await
        .map_err(|_| error_code::INTERNAL)??;
    Ok(format!("sso={grant}"))
}

async fn link(
    state: &AppState,
    user_id: &str,
    identity: &Identity,
    ip: &str,
) -> Result<String, &'static str> {
    let now = state.clock.now();
    let (user_id, identity, ip) = (user_id.to_owned(), identity.clone(), ip.to_owned());
    let linked = state
        .db
        .call(move |c| {
            let linked = factors::link_sso(
                c,
                &user_id,
                &identity.issuer,
                &identity.subject,
                &identity.label(),
                now,
            )?;
            if linked {
                store::audit(
                    c,
                    now,
                    &Event {
                        name: "sso_linked",
                        user_id: Some(&user_id),
                        ip: Some(&ip),
                        detail: Some(&identity.label()),
                        ..Event::default()
                    },
                )?;
            }
            Ok(linked)
        })
        .await
        .map_err(|_| error_code::INTERNAL)?;
    if linked {
        Ok("sso-linked".into())
    } else {
        Err(error_code::SSO_TAKEN)
    }
}

/// The account for an identity, made on the spot when the server allows
/// it, and a code to trade for its session.
async fn grant(
    state: &AppState,
    identity: &Identity,
    ip: &str,
    binding: Vec<u8>,
) -> Result<Result<String, &'static str>, Error> {
    let now = state.clock.now();
    // an account made here has no password, it cannot be end-to-end encrypted
    let create = state.config.sso.create_accounts
        && state.db.call(|c| e2ee::account_kinds(c)).await? != AccountKinds::E2ee;
    let (identity, ip) = (identity.clone(), ip.to_owned());
    let granted = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let user_id = match factors::sso_user(&tx, &identity.issuer, &identity.subject)? {
                Some(id) => id,
                None if create => {
                    let Some(user) = create_account(&tx, &identity, now)? else {
                        return Ok(Err(error_code::SSO_FAILED));
                    };
                    store::audit(
                        &tx,
                        now,
                        &Event {
                            name: "registered",
                            user_id: Some(&user),
                            ip: Some(&ip),
                            detail: Some("single sign-on"),
                            ..Event::default()
                        },
                    )?;
                    user
                }
                None => return Ok(Err(error_code::SSO_NO_ACCOUNT)),
            };
            let Some(user) = store::user_by_id(&tx, &user_id)? else {
                return Ok(Err(error_code::SSO_NO_ACCOUNT));
            };
            if user.disabled_at.is_some() {
                return Ok(Err(error_code::ACCOUNT_DISABLED));
            }
            let grant = factors::create_challenge(
                &tx,
                ChallengeKind::SsoGrant,
                Some(&user_id),
                &Granted {
                    label: identity.label(),
                    binding,
                },
                now + GRANT_LIFE,
            )?;
            tx.commit()?;
            Ok(Ok(grant))
        })
        .await?;
    Ok(granted)
}

/// A new account for an identity, with a free username close to the
/// provider's and no password. `None` when no username is free.
fn create_account(
    conn: &rusqlite::Connection,
    identity: &Identity,
    now: i64,
) -> Result<Option<String>, crate::db::DbError> {
    let base = sso_username(identity.username.as_deref(), identity.email.as_deref());
    for n in 1..100 {
        let candidate = if n == 1 {
            base.clone()
        } else {
            let suffix = format!("-{n}");
            let keep = 32 - suffix.len();
            format!("{}{suffix}", &base[..base.len().min(keep)])
        };
        let display = display_name(identity.name.as_deref(), &candidate);
        if let Some(user) = store::insert_user(conn, &candidate, &display, "", Role::User, now)? {
            factors::link_sso(
                conn,
                &user.id,
                &identity.issuer,
                &identity.subject,
                &identity.label(),
                now,
            )?;
            return Ok(Some(user.id));
        }
    }
    Ok(None)
}

/// `POST /api/v1/auth/sso/exchange`
pub async fn exchange(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: axum::http::HeaderMap,
    Body(req): Body<SsoExchangeRequest>,
) -> Result<Response, Error> {
    rate_limit(&state, ip)?;
    let now = state.clock.now();
    let hash = token::hash(req.code.trim());
    let sent = sent_binding(&headers).map(|b| token::hash(&b));
    let found = state
        .db
        .call(move |c| {
            let grant = factors::challenge::<Granted>(c, &hash, ChallengeKind::SsoGrant, now)?;
            if !factors::end_challenge(c, &hash)? {
                return Ok(None);
            }
            let Some(grant) = grant else {
                return Ok(None);
            };
            if !sent.is_some_and(|b| same(&b, &grant.data.binding)) {
                tracing::warn!("a single sign-on code exchanged in another browser");
                return Ok(None);
            }
            let user = match grant.user_id.as_deref() {
                Some(id) => store::user_by_id(c, id)?,
                None => None,
            };
            Ok(user.map(|u| (u, grant.data.label)))
        })
        .await?;
    let (user, label) = found.ok_or_else(failed)?;
    not_disabled(&user)?;
    let device = device_info(&req.device);
    // an admin signing in by the provider gives this server's second
    // factor too: the provider alone does not make someone admin here
    // Sessions already open renew as before.
    if user.role == Role::Admin
        && let Some(needed) = super::second_factor::ask(&state, &user, device.clone(), now).await?
    {
        return Ok((StatusCode::ACCEPTED, Json(needed)).into_response());
    }
    let session = finish_sign_in(
        &state,
        user,
        device,
        &ip.0.to_string(),
        Some(format!("single sign-on, {label}")),
    )
    .await?;
    Ok(Json(session).into_response())
}

/// The identities linked to an account, for the security page.
pub(super) async fn list(state: &AppState, user_id: &str) -> Result<Vec<SsoIdentity>, Error> {
    let id = user_id.to_owned();
    let rows = state
        .db
        .call(move |c| factors::sso_identities(c, &id))
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| SsoIdentity {
            id: r.id,
            name: r.name,
            created_at: r.created_at,
        })
        .collect())
}

/// `DELETE /api/v1/me/sso/{id}`. An account without a password keeps its
/// last identity, it would have no way back in.
pub async fn unlink(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(identity): Path<String>,
    Body(req): Body<Confirm>,
) -> Result<StatusCode, Error> {
    confirm(&state, &me, req, ip).await?;
    let id = me.user.id.clone();
    let passwordless = me.user.password_hash.is_empty() && me.user.kind != AccountKind::E2ee;
    let outcome = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            if passwordless && factors::sso_identities(&tx, &id)?.len() <= 1 {
                return Ok(Err(Error::new(
                    StatusCode::CONFLICT,
                    error_code::INVALID,
                    "this is the only way into the account",
                )));
            }
            let removed = factors::unlink_sso(&tx, &id, &identity)?;
            tx.commit()?;
            Ok(if removed {
                Ok(())
            } else {
                Err(Error::not_found())
            })
        })
        .await?;
    outcome?;
    log(&state, &me, ip, "sso_unlinked").await?;
    Ok(StatusCode::NO_CONTENT)
}
