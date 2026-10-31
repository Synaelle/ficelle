// SPDX-License-Identifier: AGPL-3.0-or-later

//! Http routes and the layers every response goes through: the protocol
//! version check, security headers and the body size limit.

use std::sync::Arc;

use app_protocol::{
    Compatibility, MIN_PROTOCOL_VERSION, PROTOCOL_HEADER, PROTOCOL_VERSION, ServerInfo, error_code,
};
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};

use crate::accounts::password::Passwords;
use crate::accounts::ratelimit::RateLimit;
use crate::clock::Clock;
use crate::config::Config;
use crate::db::Db;
use crate::error::Error;
use webauthn_rs::Webauthn;

/// What every handler can reach.
#[derive(Clone)]
pub struct AppState {
    /// Boot configuration.
    pub config: Arc<Config>,
    /// The database.
    pub db: Db,
    /// Password hashing with the configured cost.
    pub passwords: Passwords,
    /// Per address limit on the sign-in endpoints.
    pub rate_limit: RateLimit,
    /// Wall clock.
    pub clock: Clock,
    /// Passkey checks, `None` without `server.public_url`.
    pub webauthn: Option<Arc<Webauthn>>,
    /// Single sign-on provider, `None` when it is off.
    pub sso: Option<Arc<crate::sso::Provider>>,
    /// Outgoing email.
    pub mailer: crate::mail::Mailer,
    /// Wraps the space keys of standard accounts.
    pub master: Arc<crate::keys::MasterKey>,
    /// The OPAQUE setup, read from the database on first use.
    pub opaque: Arc<tokio::sync::OnceCell<Arc<app_e2ee::server::Setup>>>,
    /// Devices listening for changes.
    pub live: Arc<crate::live::Live>,
    /// Where attachment files are kept.
    pub files: Arc<dyn crate::files::Storage>,
    /// Held while an upload part is taken in and its file completes, and
    /// while the sweep deletes objects: no part lands while a file is
    /// checked, and a file being taken in again is never deleted under it.
    pub files_lock: Arc<tokio::sync::Mutex<()>>,
    /// Feedback bodies being read, before their proof of work is checked.
    pub feedback_reading: Arc<tokio::sync::Semaphore>,
    /// Password hashes running at once.
    pub hashing: Arc<tokio::sync::Semaphore>,
    /// The official catalog as last fetched.
    pub catalog: Arc<crate::catalog::Cache>,
    /// Feedback reports, a database of their own.
    pub feedback: Db,
    /// Feedback challenges a minute from one address.
    pub feedback_limit: RateLimit,
    /// The setup link and session, until setup is locked.
    pub setup: Arc<crate::setup::Setup>,
    /// The release list as last fetched.
    pub updates: Arc<crate::updates::Updates>,
}

impl AppState {
    /// State for a loaded config and an open database.
    pub fn new(config: Config, db: Db) -> Self {
        Self {
            passwords: Passwords::new(&config.auth),
            rate_limit: RateLimit::new(config.auth.rate_limit_per_minute),
            webauthn: crate::passkeys::relying_party(&config),
            sso: crate::sso::Provider::new(&config).map(Arc::new),
            opaque: Arc::default(),
            live: Arc::default(),
            files: Arc::new(crate::files::Memory::default()),
            files_lock: Arc::default(),
            feedback_reading: Arc::new(tokio::sync::Semaphore::new(
                crate::feedback::READING_AT_ONCE,
            )),
            hashing: Arc::new(tokio::sync::Semaphore::new(
                std::thread::available_parallelism().map_or(2, |n| n.get().clamp(2, 8)),
            )),
            mailer: crate::mail::Mailer::new(&config),
            master: Arc::new(crate::keys::MasterKey::generate()),
            catalog: Arc::new(crate::catalog::Cache::new(
                config.catalog.clone(),
                &config.storage.data_dir,
            )),
            feedback: Db::feedback_in_memory().expect("the feedback migrations apply"),
            feedback_limit: RateLimit::new(FEEDBACK_CHALLENGES_PER_MINUTE),
            setup: Arc::default(),
            updates: Arc::new(crate::updates::Updates::new(config.updates.clone())),
            config: Arc::new(config),
            db,
            clock: Clock::default(),
        }
    }
}

/// Challenges a minute from one address: plenty for people, the work and
/// the report limits do the rest.
const FEEDBACK_CHALLENGES_PER_MINUTE: u32 = 10;

/// Feedback routes answer any origin: another server's web app sends here.
const FEEDBACK_PATH: &str = "/api/v1/feedback";

/// The only api call that works without the protocol header.
const INFO_PATH: &str = "/api/v1/info";

/// The setup page's WebSocket probe: a browser cannot add headers to a
/// WebSocket, and the probe speaks no protocol.
const SETUP_SOCKET_PATH: &str = "/api/v1/setup/socket";

/// Builds the full application router.
pub fn router(state: AppState) -> Router {
    let max_body =
        usize::try_from(state.config.server.max_body_mb).unwrap_or(usize::MAX) * 1024 * 1024;
    let hsts = state.config.hsts();
    let base = state.config.base_path();
    let app = Router::new()
        .route(INFO_PATH, get(info))
        .route("/health", get(health))
        .route(crate::sso::CALLBACK_PATH, get(crate::api::sso::callback))
        .nest("/api/v1", crate::api::router())
        .merge(crate::web::router())
        .fallback(|| async { Error::not_found() })
        .layer(middleware::from_fn_with_state(state.clone(), setup_pending))
        .layer(middleware::from_fn(check_protocol))
        .layer(middleware::from_fn(feedback_cors))
        .layer(middleware::from_fn(move |req, next| {
            security_headers(hsts, req, next)
        }))
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(state);
    if base.is_empty() {
        return app;
    }
    // under a path every route moves there, the layers inside still see the
    // paths they know: nesting takes the prefix off
    let admin = format!("{base}/admin");
    Router::new()
        .nest(&base, app)
        // nesting takes `/notes` itself, not `/notes/`
        .route(
            &format!("{base}/"),
            get(move || async move { axum::response::Redirect::temporary(&admin) }),
        )
        .fallback(|| async { Error::not_found() })
        .layer(middleware::from_fn(move |req, next| {
            security_headers(hsts, req, next)
        }))
}

async fn info() -> Json<ServerInfo> {
    Json(ServerInfo {
        name: app_brand::DISPLAY_NAME.into(),
        version: app_brand::VERSION.into(),
        min_protocol: MIN_PROTOCOL_VERSION,
        max_protocol: PROTOCOL_VERSION,
    })
}

/// For load balancers and container health checks: up and the database answers.
async fn health(State(state): State<AppState>) -> Result<&'static str, Error> {
    state
        .db
        .call(|c| Ok(c.query_row("SELECT 1", [], |_| Ok(()))?))
        .await?;
    Ok("ok")
}

/// Every api call names the protocol it speaks. A mismatch gets a clear
/// answer saying who has to update, before any handler runs.
async fn check_protocol(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if !path.starts_with("/api/") || path == INFO_PATH || path == SETUP_SOCKET_PATH {
        return next.run(req).await;
    }
    let version = req
        .headers()
        .get(PROTOCOL_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u32>().ok());
    let Some(version) = version else {
        return Error::new(
            StatusCode::BAD_REQUEST,
            error_code::PROTOCOL_MISSING,
            format!("send the {PROTOCOL_HEADER} header"),
        )
        .into_response();
    };
    let range = ServerInfo {
        name: String::new(),
        version: String::new(),
        min_protocol: MIN_PROTOCOL_VERSION,
        max_protocol: PROTOCOL_VERSION,
    };
    let refused = |code, side: &str| {
        Error::new(
            StatusCode::UPGRADE_REQUIRED,
            code,
            format!(
                "protocol {version} is not supported, this server speaks {MIN_PROTOCOL_VERSION} to {PROTOCOL_VERSION}: update the {side}"
            ),
        )
        .into_response()
    };
    match range.compatibility(version) {
        Compatibility::Compatible => next.run(req).await,
        Compatibility::UpdateApp => refused(error_code::UPDATE_APP, "app"),
        Compatibility::UpdateServer => refused(error_code::UPDATE_SERVER, "server"),
    }
}

/// Until setup is locked, the api answers nothing but what the setup page
/// and the apps' first look need. The pages themselves load.
async fn setup_pending(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let open = (!path.starts_with("/api/") && path != crate::sso::CALLBACK_PATH)
        || path == INFO_PATH
        || path == "/api/v1/auth/options"
        || path == "/api/v1/privacy"
        || path == "/api/v1/setup"
        || path.starts_with("/api/v1/setup/");
    if open || state.setup.is_done() {
        return next.run(req).await;
    }
    // the command line may have made the first admin while this runs
    match state.db.call(|c| crate::setup::is_locked(c)).await {
        Ok(true) => {
            state.setup.set_done();
            next.run(req).await
        }
        Ok(false) => Error::new(
            StatusCode::SERVICE_UNAVAILABLE,
            error_code::SETUP_PENDING,
            "this server is being set up, its admin has the setup link",
        )
        .into_response(),
        Err(err) => Error::from(err).into_response(),
    }
}

/// Lets any origin's web app call the feedback routes, without credentials
/// (none are read there). The preflight is answered before the protocol
/// check, browsers send it without custom headers.
async fn feedback_cors(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let ours = path == FEEDBACK_PATH || path.starts_with("/api/v1/feedback/");
    if !ours {
        return next.run(req).await;
    }
    let mut response = if req.method() == axum::http::Method::OPTIONS {
        let mut preflight = StatusCode::NO_CONTENT.into_response();
        let headers = preflight.headers_mut();
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type, x-protocol-version"),
        );
        headers.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("86400"),
        );
        preflight
    } else {
        next.run(req).await
    };
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("retry-after"),
    );
    // read from other origins, the default same-origin would block it
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("cross-origin"),
    );
    response
}

/// Headers for an api: nothing embeds it, nothing sniffs it, nothing caches
/// it. The web app, when it arrives, sets its own content policy.
async fn security_headers(hsts: bool, req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    let mut set = |name: HeaderName, value: &'static str| {
        headers
            .entry(name)
            .or_insert(HeaderValue::from_static(value));
    };
    set(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(header::X_FRAME_OPTIONS, "DENY");
    set(header::REFERRER_POLICY, "no-referrer");
    set(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'none'; frame-ancestors 'none'",
    );
    set(header::CACHE_CONTROL, "no-store");
    set(
        HeaderName::from_static("cross-origin-resource-policy"),
        "same-origin",
    );
    if hsts {
        // this host only: the other names of the domain are not this
        // server's to decide
        set(header::STRICT_TRANSPORT_SECURITY, "max-age=31536000");
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    fn app(config: Config) -> Router {
        let state = AppState::new(config, Db::in_memory().unwrap());
        state.setup.set_done();
        router(state)
    }

    async fn send(
        app: Router,
        req: Request<Body>,
    ) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
        let response = app.oneshot(req).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let json = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
        (status, headers, json)
    }

    fn get_with(path: &str, version: Option<&str>) -> Request<Body> {
        let mut req = Request::get(path);
        if let Some(v) = version {
            req = req.header(PROTOCOL_HEADER, v);
        }
        req.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn info_advertises_protocol_range_without_the_header() {
        let (status, _, json) = send(app(Config::default()), get_with(INFO_PATH, None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["maxProtocol"], PROTOCOL_VERSION);
        assert_eq!(json["minProtocol"], MIN_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn api_calls_need_a_protocol_the_server_speaks() {
        let path = "/api/v1/anything";
        let (status, _, json) = send(app(Config::default()), get_with(path, None)).await;
        assert_eq!(
            (status, json["error"].as_str()),
            (StatusCode::BAD_REQUEST, Some("protocolMissing"))
        );

        let (status, _, json) = send(app(Config::default()), get_with(path, Some("0"))).await;
        assert_eq!(
            (status, json["error"].as_str()),
            (StatusCode::UPGRADE_REQUIRED, Some("updateApp"))
        );

        let newer = (PROTOCOL_VERSION + 1).to_string();
        let (status, _, json) = send(app(Config::default()), get_with(path, Some(&newer))).await;
        assert_eq!(
            (status, json["error"].as_str()),
            (StatusCode::UPGRADE_REQUIRED, Some("updateServer"))
        );

        let current = PROTOCOL_VERSION.to_string();
        let (status, _, json) = send(app(Config::default()), get_with(path, Some(&current))).await;
        assert_eq!(
            (status, json["error"].as_str()),
            (StatusCode::NOT_FOUND, Some("notFound")),
            "passes the check"
        );
    }

    #[tokio::test]
    async fn every_response_carries_the_security_headers() {
        let (_, headers, _) = send(app(Config::default()), get_with("/health", None)).await;
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(headers["x-frame-options"], "DENY");
        assert_eq!(headers["cache-control"], "no-store");
        assert!(
            headers.get("strict-transport-security").is_none(),
            "no HSTS without https"
        );

        let mut https = Config::default();
        https.server.public_url = "https://notes.example.org".into();
        let (status, headers, _) = send(app(https), get_with("/health", None)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            headers["strict-transport-security"]
                .to_str()
                .unwrap()
                .starts_with("max-age=")
        );
    }

    #[tokio::test]
    async fn bodies_over_the_limit_are_refused() {
        let mut small = Config::default();
        small.server.max_body_mb = 1;
        let app = small_app_with_echo(small);
        let big = vec![b'x'; 2 * 1024 * 1024];
        let req = Request::post("/echo").body(Body::from(big)).unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// The router plus a route that reads its body, which applies the limit.
    fn small_app_with_echo(config: Config) -> Router {
        let max = usize::try_from(config.server.max_body_mb).unwrap() * 1024 * 1024;
        Router::new()
            .route(
                "/echo",
                axum::routing::post(
                    |body: axum::body::Bytes| async move { body.len().to_string() },
                ),
            )
            .layer(DefaultBodyLimit::max(max))
            .merge(app(config))
    }
}
