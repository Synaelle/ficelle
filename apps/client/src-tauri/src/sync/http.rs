// SPDX-License-Identifier: AGPL-3.0-or-later

//! Calls to the server's api: JSON in and out, the protocol header, errors
//! sorted into what the sync loop and the sign-in screens need to know.
//! Blocking on purpose, it runs on the sync thread or a blocking task.

use std::time::Duration;

use app_protocol::{ApiError, PROTOCOL_HEADER, PROTOCOL_VERSION, ServerInfo};
use serde::Serialize;
use serde::de::DeserializeOwned;
use ureq::Agent;

/// Why a call failed.
#[derive(Debug, Clone)]
pub enum HttpError {
    /// No answer: no network, a name that does not resolve, a timeout.
    Offline(String),
    /// The server's certificate does not check out: someone may stand in
    /// between. Never a reason to try plain http.
    Certificate(String),
    /// The server answered with an error.
    Api {
        /// HTTP status.
        status: u16,
        /// Its body, or a stand-in when it was not one of ours.
        body: ApiError,
    },
}

impl HttpError {
    pub fn code(&self) -> &str {
        match self {
            Self::Offline(_) => "offline",
            Self::Certificate(_) => "certificate",
            Self::Api { body, .. } => &body.error,
        }
    }
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Offline(why) => write!(f, "cannot reach the server: {why}"),
            Self::Certificate(why) => {
                write!(f, "the server's certificate does not check out: {why}")
            }
            Self::Api { status, body } => write!(f, "{status} {}: {}", body.error, body.message),
        }
    }
}

/// One server.
#[derive(Clone)]
pub struct Api {
    base: String,
    agent: Agent,
    /// For files: no overall time limit, a large file takes what it takes,
    /// but a connection that stops moving still ends.
    files: Agent,
}

/// What a call answered: the body and its status, for the calls whose
/// success comes in two shapes (a session, or a second factor to give).
pub struct Answer {
    pub status: u16,
    pub body: serde_json::Value,
}

impl Answer {
    pub fn parse<T: DeserializeOwned>(self) -> Result<T, HttpError> {
        serde_json::from_value(self.body).map_err(|e| HttpError::Api {
            status: self.status,
            body: ApiError {
                error: "invalidAnswer".into(),
                message: e.to_string(),
                retry_after_seconds: None,
            },
        })
    }
}

/// A server address as typed: `notes.example.org`, with or without the
/// scheme, a trailing slash or the api path. Plain http only for this
/// machine and private networks, where a home server often has no
/// certificate.
pub fn normalize(address: &str) -> Result<String, &'static str> {
    let typed = address.trim();
    if typed.is_empty() {
        return Err("empty");
    }
    let mut text = if typed.contains("://") {
        typed.to_owned()
    } else {
        format!("https://{typed}")
    };
    while text.ends_with('/') && !text.ends_with("://") {
        text.pop();
    }
    if let Some(rest) = text.strip_suffix("/api/v1") {
        let rest = rest.len();
        text.truncate(rest);
    }
    let (scheme, rest) = text.split_once("://").ok_or("invalid")?;
    let host = match rest.strip_prefix('[') {
        // an IPv6 address: [::1]:8080
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => rest.split(['/', ':']).next().unwrap_or_default(),
    };
    if host.is_empty() || rest.contains(char::is_whitespace) {
        return Err("invalid");
    }
    match scheme {
        "https" => Ok(text),
        "http" if is_private(host) => Ok(text),
        "http" => Err("insecure"),
        _ => Err("invalid"),
    }
}

fn is_private(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let named = |suffix: &str| host.rsplit_once('.').is_some_and(|(_, end)| end == suffix);
    if host == "localhost" || named("local") || named("localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
        }
        Err(_) => false,
    }
}

impl Api {
    pub fn new(base: &str) -> Self {
        let agent: Agent = Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .user_agent(format!("{}/{}", app_brand::SLUG, app_brand::VERSION))
            .build()
            .into();
        let files: Agent = Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(None)
            .timeout_per_read(Some(Duration::from_secs(60)))
            .timeout_per_write(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .user_agent(format!("{}/{}", app_brand::SLUG, app_brand::VERSION))
            .build()
            .into();
        Self {
            base: base.trim_end_matches('/').to_owned(),
            agent,
            files,
        }
    }

    /// Sends raw bytes with extra headers: one part of a file.
    pub fn put_bytes(
        &self,
        path: &str,
        token: &str,
        headers: &[(&str, String)],
        bytes: &[u8],
    ) -> Result<Answer, HttpError> {
        let mut request = self
            .files
            .put(format!("{}/api/v1{path}", self.base))
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/octet-stream");
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        read(request.send(bytes).map_err(offline)?)
    }

    /// A file's content as a stream, read as it arrives.
    pub fn get_stream(
        &self,
        path: &str,
        token: &str,
    ) -> Result<Box<dyn std::io::Read + Send>, HttpError> {
        let response = self
            .files
            .get(format!("{}/api/v1{path}", self.base))
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
            .header("authorization", format!("Bearer {token}"))
            .call()
            .map_err(offline)?;
        if response.status().is_success() {
            let reader = response
                .into_body()
                .into_with_config()
                .limit(u64::MAX)
                .reader();
            return Ok(Box::new(reader));
        }
        read(response).map(|_| -> Box<dyn std::io::Read + Send> { Box::new(std::io::empty()) })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// The server's name and protocol range, the one call without the
    /// protocol header.
    pub fn info(&self) -> Result<ServerInfo, HttpError> {
        let response = self
            .agent
            .get(format!("{}/api/v1/info", self.base))
            .call()
            .map_err(offline)?;
        read(response)?.parse()
    }

    pub fn get(&self, path: &str, token: Option<&str>) -> Result<Answer, HttpError> {
        let mut request = self
            .agent
            .get(format!("{}/api/v1{path}", self.base))
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string());
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        read(request.call().map_err(offline)?)
    }

    pub fn post(
        &self,
        path: &str,
        token: Option<&str>,
        body: &impl Serialize,
    ) -> Result<Answer, HttpError> {
        let mut request = self
            .agent
            .post(format!("{}/api/v1{path}", self.base))
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string());
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        read(request.send_json(body).map_err(offline)?)
    }

    pub fn put(
        &self,
        path: &str,
        token: Option<&str>,
        body: &impl Serialize,
    ) -> Result<Answer, HttpError> {
        let mut request = self
            .agent
            .put(format!("{}/api/v1{path}", self.base))
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string());
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        read(request.send_json(body).map_err(offline)?)
    }

    pub fn delete(&self, path: &str, token: Option<&str>) -> Result<Answer, HttpError> {
        let mut request = self
            .agent
            .delete(format!("{}/api/v1{path}", self.base))
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string());
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        read(request.call().map_err(offline)?)
    }
}

#[expect(clippy::needless_pass_by_value, reason = "used as map_err(offline)")]
fn offline(err: ureq::Error) -> HttpError {
    let text = err.to_string();
    let tls = matches!(err, ureq::Error::Tls(_) | ureq::Error::Rustls(_))
        || text.to_ascii_lowercase().contains("certificate");
    if tls {
        HttpError::Certificate(text)
    } else {
        HttpError::Offline(text)
    }
}

fn read(mut response: ureq::http::Response<ureq::Body>) -> Result<Answer, HttpError> {
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_string()
        .map_err(offline)?;
    let body: serde_json::Value = if text.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text))
    };
    if (200..300).contains(&status) {
        return Ok(Answer { status, body });
    }
    let body = serde_json::from_value::<ApiError>(body.clone()).unwrap_or_else(|_| ApiError {
        error: "httpError".into(),
        message: format!("{status}: not an answer of this server ({body})"),
        retry_after_seconds: None,
    });
    Err(HttpError::Api { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_read_like_people_type_them() {
        assert_eq!(
            normalize("notes.example.org").unwrap(),
            "https://notes.example.org"
        );
        assert_eq!(
            normalize(" https://notes.example.org/api/v1/ ").unwrap(),
            "https://notes.example.org"
        );
        assert_eq!(
            normalize("http://localhost:8080").unwrap(),
            "http://localhost:8080"
        );
        assert_eq!(
            normalize("http://192.168.1.20:8080").unwrap(),
            "http://192.168.1.20:8080"
        );
        assert_eq!(normalize("http://[::1]:8080").unwrap(), "http://[::1]:8080");
        // an instance under a path keeps it, with or without the api part
        assert_eq!(
            normalize("example.org/notes/").unwrap(),
            "https://example.org/notes"
        );
        assert_eq!(
            normalize("https://example.org/notes/api/v1").unwrap(),
            "https://example.org/notes"
        );
        assert_eq!(normalize("http://nas.local").unwrap(), "http://nas.local");
        assert_eq!(normalize("http://notes.example.org"), Err("insecure"));
        assert_eq!(normalize("http://8.8.8.8"), Err("insecure"));
        assert_eq!(normalize("ftp://x"), Err("invalid"));
        assert_eq!(normalize("https://"), Err("invalid"));
        assert_eq!(normalize("   "), Err("empty"));
    }
}
