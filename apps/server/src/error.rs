// SPDX-License-Identifier: AGPL-3.0-or-later

//! Failed requests as `{ "error": code, "message": text }` with the right
//! status. Internal details go to the log, never to the response.

use app_protocol::{ApiError, error_code};
use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::db::DbError;

/// An error a handler returns.
#[derive(Debug)]
pub struct Error {
    status: StatusCode,
    code: &'static str,
    message: String,
    retry_after: Option<u64>,
}

impl Error {
    /// A known failure the client can act on.
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retry_after: None,
        }
    }

    /// Adds a wait in seconds, sent as `Retry-After` and in the body.
    #[must_use]
    pub fn retry_after(mut self, seconds: u64) -> Self {
        self.retry_after = Some(seconds);
        self
    }

    /// No such endpoint.
    pub fn not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            error_code::NOT_FOUND,
            "no such endpoint",
        )
    }

    /// A request the endpoint cannot use, `message` says why.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error_code::INVALID, message)
    }

    /// No valid access token.
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            error_code::UNAUTHORIZED,
            "sign in again",
        )
    }

    /// Signed in, but not allowed.
    pub fn forbidden() -> Self {
        Self::new(StatusCode::FORBIDDEN, error_code::FORBIDDEN, "not allowed")
    }

    /// Something broke, the details go to the log.
    pub fn internal(detail: &str) -> Self {
        tracing::error!("{detail}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            error_code::INTERNAL,
            "internal error",
        )
    }
}

impl From<DbError> for Error {
    fn from(err: DbError) -> Self {
        Self::internal(&err.to_string())
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let body = ApiError {
            error: self.code.into(),
            message: self.message,
            retry_after_seconds: self.retry_after,
        };
        let mut response = (self.status, Json(body)).into_response();
        if let Some(seconds) = self.retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}
