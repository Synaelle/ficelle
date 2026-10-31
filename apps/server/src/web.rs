// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin page and its files, from `server.web_dir` (the web build of the
//! client). `/admin` is the page, `/invite/<token>` the same page showing a
//! sign-up form, `/assets/...` its scripts, styles and fonts.
//! `/` leads to the admin page until the web app for notes exists. Without
//! `web_dir` these paths answer 404 like any unknown path.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;

use crate::error::Error;
use crate::routes::AppState;

/// For pages: scripts, styles and data only from this server, nothing may
/// frame the page. Inline styles are allowed, Svelte sets style attributes.
/// `wasm-unsafe-eval` lets the page compile its own WebAssembly (the e2ee
/// client), it allows no JavaScript eval.
const PAGE_POLICY: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
    img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; object-src 'none'; \
    base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

/// Web routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(to_admin))
        .route("/admin", get(admin_page))
        // an invite link opens the same page, which shows a sign-up form
        .route("/invite/{token}", get(admin_page))
        // links from mails and the public pages, all handled by the same page
        .route("/verify/{token}", get(admin_page))
        .route("/reset/{token}", get(admin_page))
        .route("/recover", get(admin_page))
        .route("/privacy", get(admin_page))
        // the setup link, its token after `#` stays in the browser
        .route("/setup", get(admin_page))
        .route("/assets/{file}", get(asset))
        // the fonts the apps download, mirrored at build (tools/fonts-mirror.mjs)
        .route("/fonts/{font}/{file}", get(font))
}

fn web_dir(state: &AppState) -> Result<&Path, Error> {
    state
        .config
        .server
        .web_dir
        .as_deref()
        .filter(|d| !d.as_os_str().is_empty())
        .ok_or_else(Error::not_found)
}

async fn to_admin(State(state): State<AppState>) -> Redirect {
    Redirect::temporary(&format!("{}/admin", state.config.base_path()))
}

/// The page, told where it lives: its assets and every address it builds
/// are relative to the `<base>` added here, so it works under any path.
async fn admin_page(State(state): State<AppState>) -> Result<Response, Error> {
    let path = web_dir(&state)?.join("admin.html");
    let html = tokio::fs::read_to_string(&path)
        .await
        .map_err(|_| Error::not_found())?;
    let base = format!(
        "<head>\n    <base href=\"{}/\" />",
        state.config.base_path()
    );
    let html = html.replacen("<head>", &base, 1);
    Ok(respond(
        html.into_bytes(),
        "text/html; charset=utf-8",
        "no-cache",
    ))
}

/// Built file names carry a content hash, so they never change: browsers
/// may keep them for a year. Only plain names are served, nothing with a
/// slash or a leading dot can leave the folder.
async fn asset(
    State(state): State<AppState>,
    UrlPath(file): UrlPath<String>,
) -> Result<Response, Error> {
    let plain = !file.starts_with('.')
        && file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !plain {
        return Err(Error::not_found());
    }
    let path: PathBuf = web_dir(&state)?.join("assets").join(&file);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| Error::not_found())?;
    Ok(respond(
        bytes,
        content_type(&file),
        "public, max-age=31536000, immutable",
    ))
}

/// A mirrored font file: `<font>/<name>.woff2`, the font a Fontsource id
/// (`@vf` for a variable one), the name letters, digits and dashes. Nothing
/// else is served, nothing can leave the folder.
async fn font(
    State(state): State<AppState>,
    UrlPath((font, file)): UrlPath<(String, String)>,
) -> Result<Response, Error> {
    let font_ok = !font.is_empty()
        && font.len() <= 64
        && font
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '@'));
    let name = file.strip_suffix(".woff2").unwrap_or_default();
    let file_ok = !name.is_empty()
        && name.len() <= 32
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !font_ok || !file_ok {
        return Err(Error::not_found());
    }
    let path: PathBuf = web_dir(&state)?.join("fonts").join(&font).join(&file);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| Error::not_found())?;
    // a font's files change only when the mirror runs again with a newer
    // image: a week is plenty
    Ok(respond(bytes, "font/woff2", "public, max-age=604800"))
}

fn respond(body: Vec<u8>, kind: &'static str, cache: &'static str) -> Response {
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    // set here so the api's stricter default does not replace it
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(PAGE_POLICY),
    );
    response
}

fn content_type(file: &str) -> &'static str {
    match file.rsplit('.').next().unwrap_or("") {
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "json" => "application/json",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::config::Config;
    use crate::db::Db;
    use crate::routes::router as app;

    fn site() -> (tempfile::TempDir, axum::Router) {
        site_under(None)
    }

    fn site_under(base: Option<&str>) -> (tempfile::TempDir, axum::Router) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("assets")).unwrap();
        std::fs::write(
            dir.path().join("admin.html"),
            "<!doctype html><head><title>admin</title></head>",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("assets").join("admin-abc123.js"),
            "console.log(1)",
        )
        .unwrap();
        std::fs::write(dir.path().join("secret.txt"), "no").unwrap();
        let mut config = Config::default();
        config.server.web_dir = Some(dir.path().to_path_buf());
        config.server.base_path = base.map(str::to_owned);
        let state = AppState::new(config, Db::in_memory().unwrap());
        state.setup.set_done();
        (dir, app(state))
    }

    #[tokio::test]
    async fn mirrored_fonts_are_served_and_nothing_beside_them() {
        let (dir, app) = site();
        let inter = dir.path().join("fonts").join("inter");
        std::fs::create_dir_all(&inter).unwrap();
        std::fs::write(inter.join("latin-400-normal.woff2"), b"wOF2 font").unwrap();
        let (status, headers, body) = get(&app, "/fonts/inter/latin-400-normal.woff2").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "font/woff2");
        assert_eq!(body, "wOF2 font");
        for bad in [
            "/fonts/inter/latin-700-normal.woff2",
            "/fonts/inter/secret.txt",
            "/fonts/..%2F/secret.txt",
            "/fonts/Inter/latin-400-normal.woff2",
        ] {
            let (status, _, _) = get(&app, bad).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{bad}");
        }
    }

    async fn get(app: &axum::Router, path: &str) -> (StatusCode, axum::http::HeaderMap, String) {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, headers, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn the_page_and_its_files_with_a_page_policy() {
        let (_dir, app) = site();
        let (status, headers, body) = get(&app, "/admin").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("<title>admin"));
        assert!(
            headers["content-security-policy"]
                .to_str()
                .unwrap()
                .starts_with("default-src 'self'")
        );
        assert_eq!(
            headers["x-frame-options"], "DENY",
            "the shared headers still apply"
        );

        for page in [
            "/invite/inv_x",
            "/verify/ch_x",
            "/reset/ch_x",
            "/recover",
            "/privacy",
        ] {
            assert_eq!(get(&app, page).await.0, StatusCode::OK, "{page}");
        }

        let (status, headers, _) = get(&app, "/assets/admin-abc123.js").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            headers["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/javascript")
        );
        assert!(
            headers["cache-control"]
                .to_str()
                .unwrap()
                .contains("immutable")
        );

        let (status, headers, _) = get(&app, "/").await;
        assert_eq!(
            (status, headers["location"].to_str().unwrap()),
            (StatusCode::TEMPORARY_REDIRECT, "/admin")
        );
    }

    #[tokio::test]
    async fn under_a_path_everything_moves_there() {
        let (_dir, app) = site_under(Some("/notes"));
        let (status, headers, body) = get(&app, "/notes/admin").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("<base href=\"/notes/\" />"), "{body}");
        assert!(
            headers["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("base-uri 'self'")
        );
        for path in [
            "/notes/setup",
            "/notes/invite/inv_x",
            "/notes/assets/admin-abc123.js",
        ] {
            assert_eq!(get(&app, path).await.0, StatusCode::OK, "{path}");
        }
        for root in ["/notes", "/notes/"] {
            let (status, headers, _) = get(&app, root).await;
            assert_eq!(status, StatusCode::TEMPORARY_REDIRECT, "{root}");
            assert_eq!(headers["location"], "/notes/admin");
        }
        assert_eq!(get(&app, "/notes/health").await.0, StatusCode::OK);
        assert_eq!(get(&app, "/notes/api/v1/info").await.0, StatusCode::OK);
        for outside in ["/admin", "/health", "/api/v1/info", "/notesx/admin"] {
            let (status, headers, _) = get(&app, outside).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{outside}");
            assert_eq!(headers["x-content-type-options"], "nosniff");
        }
        let (_, _, body) = get(&site().1, "/admin").await;
        assert!(body.contains("<base href=\"/\" />"), "at the root too");
    }

    #[tokio::test]
    async fn nothing_outside_the_assets_folder() {
        let (_dir, app) = site();
        for path in [
            "/assets/..%2Fsecret.txt",
            "/assets/.hidden",
            "/assets/..",
            "/secret.txt",
            "/assets/missing.js",
        ] {
            assert_eq!(get(&app, path).await.0, StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[tokio::test]
    async fn without_a_web_folder_the_page_is_not_there() {
        let app = app(AppState::new(Config::default(), Db::in_memory().unwrap()));
        assert_eq!(get(&app, "/admin").await.0, StatusCode::NOT_FOUND);
    }
}
