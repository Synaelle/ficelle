// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `attachment` URI scheme serves decrypted attachment files to the
//! webview by hash: `attachment://localhost/<sha256>` on macOS, iOS and Linux,
//! `http://attachment.localhost/<sha256>` on Windows and Android. Files never
//! touch disk in clear text and nothing is served while the vault is locked.

use tauri::http::{Request, Response, StatusCode, header};
use tauri::{Manager, Runtime, UriSchemeContext};

use crate::state::AppState;

pub const SCHEME: &str = "attachment";

/// Media type from the first bytes. Only formats shown inline are detected,
/// anything else is served as a download.
fn sniff(bytes: &[u8]) -> &'static str {
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    if starts(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if starts(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "image/gif"
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.len() > 12
        && &bytes[4..8] == b"ftyp"
        && (&bytes[8..12] == b"avif" || &bytes[8..12] == b"avis")
    {
        "image/avif"
    } else {
        "application/octet-stream"
    }
}

fn respond(status: StatusCode, mime: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        // never cached: the webview cache lives on disk outside the vault, an
        // encrypted vault must not leave decrypted copies of its files there
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .body(body)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

// the argument types are fixed by the protocol handler signature tauri expects
#[allow(clippy::needless_pass_by_value)]
pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let sha = request.uri().path().trim_start_matches('/');
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return respond(
            StatusCode::BAD_REQUEST,
            "text/plain",
            b"bad attachment address".to_vec(),
        );
    }
    let state = ctx.app_handle().state::<AppState>();
    match state.with(|s| s.blobs.get(sha)) {
        Ok(Some(bytes)) => {
            let mime = sniff(&bytes);
            respond(StatusCode::OK, mime, bytes)
        }
        Ok(None) => respond(StatusCode::NOT_FOUND, "text/plain", Vec::new()),
        Err(_) => respond(StatusCode::FORBIDDEN, "text/plain", Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::sniff;

    #[test]
    fn sniffs_common_images_only() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), "image/png");
        assert_eq!(sniff(b"\xff\xd8\xff\xe0"), "image/jpeg");
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), "image/webp");
        assert_eq!(sniff(b"<svg onload=alert(1)>"), "application/octet-stream");
    }
}
