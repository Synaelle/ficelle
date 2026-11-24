// SPDX-License-Identifier: AGPL-3.0-or-later

//! What people do with their own account besides signing in: the export of
//! their data. And the facts the privacy page shows.

use app_protocol::profile::PrivacyInfo;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::extract::{ClientIp, CurrentUser};
use super::security::log;
use crate::accounts::profile;
use crate::error::Error;
use crate::routes::AppState;

/// `GET /api/v1/me/export`: everything the server keeps for the account,
/// as a json file.
pub async fn export(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
) -> Result<Response, Error> {
    let now = state.clock.now();
    let id = me.user.id.clone();
    let data = state.db.call(move |c| profile::export(c, &id, now)).await?;
    log(&state, &me, ip, "data_exported").await?;
    let body = serde_json::to_vec_pretty(&data).map_err(|e| Error::internal(&e.to_string()))?;
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let name = format!(
        "attachment; filename=\"{}-{}.json\"",
        app_brand::SLUG,
        me.user.username
    );
    if let Ok(value) = HeaderValue::from_str(&name) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

/// `GET /api/v1/privacy`
pub async fn privacy(State(state): State<AppState>) -> Result<Json<PrivacyInfo>, Error> {
    let config = &state.config;
    let feedback_config = config.feedback.clone();
    let feedback = state
        .db
        .call(move |c| crate::feedback::settings(c, &feedback_config))
        .await?
        .info();
    let main_host = app_brand::FEEDBACK_URL
        .split("://")
        .nth(1)
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_owned();
    let given = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
    Ok(Json(PrivacyInfo {
        operator: given(&config.privacy.operator),
        contact: given(&config.privacy.contact),
        notice: given(&config.privacy.notice),
        // this server sends no email
        email: false,
        audit_days: config.auth.audit_days,
        session_days: config.auth.refresh_token_days,
        feedback_receive: feedback.receive,
        feedback_keep_days: config.feedback.keep_days,
        feedback_send_to: feedback.send_to,
        feedback_main: main_host,
    }))
}
