// SPDX-License-Identifier: AGPL-3.0-or-later

//! Feedback through the full router, as anyone's app sends it.

use std::net::SocketAddr;

use app_protocol::feedback::solve;
use app_protocol::{PROTOCOL_HEADER, PROTOCOL_VERSION};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::tests::{admin_on, call, events, server_with};
use crate::config::{Config, FeedbackConfig};
use crate::feedback::{BlockKind, block};
use crate::routes::AppState;

/// A receiving server with cheap challenges, its admin's token.
async fn receiving(feedback: FeedbackConfig) -> (AppState, Router, String) {
    let config = Config {
        feedback: FeedbackConfig {
            receive: true,
            pow_bits: 8,
            ..feedback
        },
        ..Config::default()
    };
    admin_on(server_with(config).await).await
}

/// A request from `ip`, no token: how apps send feedback.
async fn from(
    app: &Router,
    ip: &str,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
) -> (StatusCode, HeaderMap, Value) {
    from_as(app, ip, method, path, body, None).await
}

/// The same with an account's token, as an app signed in here sends it.
async fn from_as(
    app: &Router,
    ip: &str,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
    token: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(format!("/api/v1{path}"))
        .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string());
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(ip.parse().unwrap(), 40_000)));
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A solved report from `ip`, with `extra` fields over a plain one.
async fn report(app: &Router, ip: &str, extra: Value) -> (StatusCode, HeaderMap, Value) {
    report_as(app, ip, extra, None).await
}

/// The same from an app signed in with `token`.
async fn report_as(
    app: &Router,
    ip: &str,
    extra: Value,
    token: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let (status, _, challenge) = from(app, ip, "POST", "/feedback/challenge", None).await;
    assert_eq!(status, StatusCode::OK, "{challenge}");
    let text = challenge["challenge"].as_str().unwrap();
    let bits = u8::try_from(challenge["bits"].as_u64().unwrap()).unwrap();
    let mut body = json!({
        "challenge": text,
        "nonce": solve(text, bits),
        "category": "bug",
        "message": "The sidebar does not open after the update.",
        "version": "0.12.0",
        "platform": "windows",
    });
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    from_as(
        app,
        ip,
        "POST",
        "/feedback",
        Some(body.to_string().into_bytes()),
        token,
    )
    .await
}

async fn reports(state: &AppState) -> Vec<(String, String, Option<String>, i64)> {
    state
        .feedback
        .call(|c| {
            let mut stmt = c.prepare(
                "SELECT status, message, contact,
                     (SELECT count(*) FROM images WHERE report = reports.id)
                 FROM reports ORDER BY created_at, id",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn off_until_the_admin_turns_it_on() {
    let (state, app) = server_with(Config::default()).await;
    let (status, _, info) = from(&app, "10.0.0.1", "GET", "/feedback/info", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info, json!({ "receive": false, "sendTo": "main" }));
    let (status, _, body) = from(&app, "10.0.0.1", "POST", "/feedback/challenge", None).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("feedbackOff"))
    );
    let (status, _, _) = from(&app, "10.0.0.1", "POST", "/feedback", Some(b"{}".to_vec())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, app, admin) = admin_on((state.clone(), app)).await;
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/feedback/settings",
        None,
        Some(json!({ "receive": true, "sendTo": "here" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, settings) =
        call(&app, "GET", "/admin/feedback/settings", Some(&admin), None).await;
    assert_eq!(
        (status, settings),
        (
            StatusCode::OK,
            json!({
                "receive": false,
                "sendTo": "main",
                "readers": [],
                "tokenCreatedAt": null,
                "caps": { "perHour": null, "perDay": null },
                "defaultCaps": { "perHour": 20, "perDay": 100 },
            })
        )
    );
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/feedback/settings",
        Some(&admin),
        Some(json!({ "receive": true, "sendTo": "here" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        events(&state)
            .await
            .contains(&"feedback_settings".to_owned())
    );
    let (_, _, info) = from(&app, "10.0.0.1", "GET", "/feedback/info", None).await;
    assert_eq!(info, json!({ "receive": true, "sendTo": "here" }));
    let (status, _, _) = report(&app, "10.0.0.1", json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // sending here while not receiving is told as off
    call(
        &app,
        "PUT",
        "/admin/feedback/settings",
        Some(&admin),
        Some(json!({ "receive": false, "sendTo": "here" })),
    )
    .await;
    let (_, _, info) = from(&app, "10.0.0.1", "GET", "/feedback/info", None).await;
    assert_eq!(info, json!({ "receive": false, "sendTo": "off" }));
}

#[tokio::test]
async fn a_report_with_its_parts_is_kept_and_its_challenge_used_once() {
    let (state, app, admin) = receiving(FeedbackConfig::default()).await;
    let png = crate::feedback::images::tests::png_of(32, 16);
    let (status, _, _) = report(
        &app,
        "10.0.0.1",
        json!({
            "install": "0123456789abcdef0123456789abcdef",
            "email": "Kim@Example.org",
            "logs": "error: sync stopped",
            "images": [STANDARD.encode(&png)],
            "category": "idea",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let kept = reports(&state).await;
    assert_eq!(
        kept,
        [(
            "new".into(),
            "The sidebar does not open after the update.".into(),
            Some("kim@example.org".into()),
            1
        )]
    );
    let (category, logs, install): (String, String, String) = state
        .feedback
        .call(|c| {
            Ok(
                c.query_row("SELECT category, logs, install FROM reports", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?,
            )
        })
        .await
        .unwrap();
    assert_eq!(
        (category.as_str(), logs.as_str(), install.len()),
        ("idea", "error: sync stopped", 32)
    );

    // the same challenge again
    let (_, _, challenge) = from(&app, "10.0.0.2", "POST", "/feedback/challenge", None).await;
    let text = challenge["challenge"].as_str().unwrap();
    let nonce = solve(text, 8);
    let body = json!({ "challenge": text, "nonce": nonce, "category": "bug",
        "message": "first use of this challenge", "version": "1", "platform": "web" });
    let send = |body: &Value| {
        from(
            &app,
            "10.0.0.2",
            "POST",
            "/feedback",
            Some(body.to_string().into_bytes()),
        )
    };
    assert_eq!(send(&body).await.0, StatusCode::NO_CONTENT);
    let mut again = body.clone();
    again["message"] = json!("second use of this challenge");
    let (status, _, refused) = send(&again).await;
    assert_eq!(
        (status, refused["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("challengeInvalid"))
    );

    // a signed-in person's token changes nothing, nothing ties a report to them
    let mut req = Request::post("/api/v1/feedback/challenge")
        .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
        .header("authorization", format!("Bearer {admin}"))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::new("10.0.0.3".parse().unwrap(), 1)));
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::OK
    );
    let columns: Vec<String> = state
        .feedback
        .call(|c| {
            let mut stmt = c.prepare("SELECT name FROM pragma_table_info('reports')")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
        .unwrap();
    assert!(
        !columns
            .iter()
            .any(|c| c.contains("user") || c.contains("account"))
    );
}

#[tokio::test]
async fn a_report_from_a_newer_app_says_what_it_is_about() {
    let (_, app, admin) = receiving(FeedbackConfig::default()).await;
    let (status, _, _) = report(
        &app,
        "10.0.0.1",
        json!({
            "category": null,
            "about": "Sync stops after locking",
            "message": "",
            "contact": "@Synaelle",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, page) = call(&app, "GET", "/admin/feedback/reports", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let listed = &page["reports"][0];
    assert_eq!(listed["about"], "Sync stops after locking");
    assert_eq!(listed["category"], "");
    assert_eq!(listed["hasContact"], true);
    let id = listed["id"].as_str().unwrap();
    let (status, full) = call(
        &app,
        "GET",
        &format!("/admin/feedback/reports/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{full}");
    assert_eq!(full["contact"], "synaelle");
    assert_eq!(full["message"], "");

    // a contact that is neither an address nor a Discord name
    let (status, _, body) = report(&app, "10.0.0.2", json!({ "contact": "call me" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn broken_unsolved_and_oversized_reports_are_refused() {
    let (state, app, _) = receiving(FeedbackConfig::default()).await;
    let raw = |body: Vec<u8>| from(&app, "10.0.0.1", "POST", "/feedback", Some(body));
    let (status, _, body) = raw(b"not json".to_vec()).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
    let (status, _, _) = raw(vec![b' '; 10 * 1024 * 1024]).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    let (status, _, body) = report(&app, "10.0.0.1", json!({ "nonce": 0, "challenge": "x" })).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("challengeInvalid"))
    );
    let (status, _, body) = report(&app, "10.0.0.1", json!({ "message": "" })).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid"))
    );
    let (status, _, body) = report(&app, "10.0.0.1", json!({ "images": ["R0lGODlh"] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(reports(&state).await.is_empty());

    // three images of 2 MB fit in one request, base64 and all
    let mut png = crate::feedback::images::tests::png_of(100, 100);
    png.resize(2 * 1024 * 1024, 0);
    let image = STANDARD.encode(&png);
    let (status, _, body) = report(
        &app,
        "10.0.0.1",
        json!({ "images": [image, image, image], "logs": "x".repeat(64 * 1024) }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(reports(&state).await[0].3, 3);
}

#[tokio::test]
async fn limits_answer_when_to_try_again() {
    let (_, app, _) = receiving(FeedbackConfig {
        per_hour: 2,
        ..FeedbackConfig::default()
    })
    .await;
    let install = json!({ "install": "0123456789abcdef0123456789abcdef" });
    for n in 0..2 {
        let mut body = install.clone();
        body["message"] = json!(format!("report {n} about the sidebar"));
        assert_eq!(
            report(&app, "10.0.0.1", body).await.0,
            StatusCode::NO_CONTENT
        );
    }
    let mut body = install.clone();
    body["message"] = json!("one report too many");
    let (status, headers, refused) = report(&app, "10.0.0.1", body).await;
    assert_eq!(
        (status, refused["error"].as_str()),
        (StatusCode::TOO_MANY_REQUESTS, Some("rateLimited"))
    );
    let wait: u64 = headers["retry-after"].to_str().unwrap().parse().unwrap();
    assert!(wait > 3500, "{wait}");
    // the busy address is asked for more work
    let (_, _, challenge) = from(&app, "10.0.0.1", "POST", "/feedback/challenge", None).await;
    assert_eq!(challenge["bits"], 12);

    // challenges themselves, ten a minute
    let mut statuses = Vec::new();
    for _ in 0..12 {
        statuses.push(
            from(&app, "10.0.0.9", "POST", "/feedback/challenge", None)
                .await
                .0,
        );
    }
    assert_eq!(
        statuses.iter().filter(|s| **s == StatusCode::OK).count(),
        10
    );
    assert_eq!(statuses[11], StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn caps_set_on_the_admin_page_replace_the_config_s() {
    let (_, app, admin) = receiving(FeedbackConfig {
        per_hour: 2,
        ..FeedbackConfig::default()
    })
    .await;
    let (status, view) = call(&app, "GET", "/admin/feedback/settings", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["caps"], json!({ "perHour": null, "perDay": null }));
    assert_eq!(view["defaultCaps"]["perHour"], 2);

    for refused in [json!({ "perHour": 0 }), json!({ "perDay": 100_001 })] {
        let (status, _) = call(
            &app,
            "PUT",
            "/admin/feedback/caps",
            Some(&admin),
            Some(refused),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/feedback/caps",
        None,
        Some(json!({ "perHour": 4 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, view) = call(
        &app,
        "PUT",
        "/admin/feedback/caps",
        Some(&admin),
        Some(json!({ "perHour": 4 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["caps"]["perHour"], 4);

    // four an hour now, not the config's two
    let send = |n: u32| {
        let app = app.clone();
        async move {
            let body = json!({
                "install": "0123456789abcdef0123456789abcdef",
                "message": format!("report {n} about the sidebar"),
            });
            report(&app, "10.0.0.1", body).await.0
        }
    };
    for n in 0..4 {
        assert_eq!(send(n).await, StatusCode::NO_CONTENT, "report {n}");
    }
    assert_eq!(send(4).await, StatusCode::TOO_MANY_REQUESTS);

    // a server's people share its caps, wherever they send from
    let from_server = |n: u32, ip: &'static str| {
        let app = app.clone();
        async move {
            let body = json!({
                "message": format!("report {n} from a server's person"),
                "server": "Notes.Example.org",
            });
            report(&app, ip, body).await.0
        }
    };
    for n in 0..4 {
        let ip = ["10.1.0.1", "10.1.0.2"][n as usize % 2];
        assert_eq!(
            from_server(n, ip).await,
            StatusCode::NO_CONTENT,
            "server report {n}"
        );
    }
    assert_eq!(
        from_server(4, "10.1.0.3").await,
        StatusCode::TOO_MANY_REQUESTS
    );

    // this instance's own people are not capped, and still anonymous
    for n in 0..6 {
        let body = json!({ "message": format!("report {n} from a member"), "server": "notes.example.org" });
        assert_eq!(
            report_as(&app, "10.0.0.1", body, Some(&admin)).await.0,
            StatusCode::NO_CONTENT,
            "member report {n}"
        );
    }

    // switching feedback on and off keeps the caps
    let (_, view) = call(
        &app,
        "PUT",
        "/admin/feedback/settings",
        Some(&admin),
        Some(json!({ "receive": true, "sendTo": "here" })),
    )
    .await;
    assert_eq!(view["caps"]["perHour"], 4);
}

#[tokio::test]
async fn blocked_senders_and_spam_are_told_the_same_as_everyone() {
    let (state, app, _) = receiving(FeedbackConfig::default()).await;
    let now = state.clock.now();
    let address = state
        .feedback
        .call(|c| crate::feedback::address_key(c, "10.0.0.66".parse().unwrap()))
        .await
        .unwrap();
    state
        .feedback
        .call(move |c| block(c, now, BlockKind::Address, &address, "", None))
        .await
        .unwrap();
    let (status, _, _) = report(&app, "10.0.0.66", json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let ads = json!({ "message": "buy https://a.example https://b.example https://c.example" });
    assert_eq!(
        report(&app, "10.0.0.7", ads).await.0,
        StatusCode::NO_CONTENT
    );
    let kept = reports(&state).await;
    assert_eq!(kept.len(), 1, "the blocked one is not kept");
    assert_eq!(kept[0].0, "spam");
}

#[tokio::test]
async fn other_origins_may_call_the_feedback_routes_only() {
    let (_, app, _) = receiving(FeedbackConfig::default()).await;
    // a browser's preflight carries no protocol header
    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/feedback")
        .header("origin", "https://notes.example.org")
        .header("access-control-request-method", "POST")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let headers = response.headers();
    assert_eq!(headers["access-control-allow-origin"], "*");
    assert!(
        headers["access-control-allow-headers"]
            .to_str()
            .unwrap()
            .contains("x-protocol-version")
    );
    assert!(headers.get("access-control-allow-credentials").is_none());

    let (_, headers, _) = from(&app, "10.0.0.1", "GET", "/feedback/info", None).await;
    assert_eq!(headers["access-control-allow-origin"], "*");
    assert_eq!(headers["cross-origin-resource-policy"], "cross-origin");
    let (_, headers, _) = from(&app, "10.0.0.1", "GET", "/admin/feedback/settings", None).await;
    assert!(headers.get("access-control-allow-origin").is_none());
    let (_, headers, _) = from(&app, "10.0.0.1", "GET", "/feedbackx", None).await;
    assert!(headers.get("access-control-allow-origin").is_none());
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "one story, read top to bottom")]
async fn admins_and_readers_moderate_the_token_only_reads() {
    use super::sharing_tests::{open_registration, standard};
    let (state, app, admin) = receiving(FeedbackConfig::default()).await;
    open_registration(&app, &admin).await;
    let kim = standard(&app, "kim").await;
    let kim = kim["accessToken"].as_str().unwrap().to_owned();
    let png = crate::feedback::images::tests::png_of(8, 8);
    let install = "0123456789abcdef0123456789abcdef";
    for (ip, message, extra) in [
        (
            "10.0.0.1",
            "The sidebar does not open on Android.",
            json!({ "platform": "android", "images": [STANDARD.encode(&png)], "install": install }),
        ),
        (
            "10.0.0.2",
            "Export to PDF loses the images.",
            json!({ "category": "idea", "email": "sam@example.org" }),
        ),
        (
            "10.0.0.3",
            "buy https://a.example https://b.example https://c.example",
            json!({}),
        ),
    ] {
        let mut extra = extra;
        extra["message"] = json!(message);
        assert_eq!(report(&app, ip, extra).await.0, StatusCode::NO_CONTENT);
    }

    // kim is nobody here yet
    let (status, access) = call(&app, "GET", "/admin/feedback/access", Some(&kim), None).await;
    assert_eq!(
        (status, access),
        (StatusCode::OK, json!({ "inbox": false, "admin": false }))
    );
    let (status, _) = call(&app, "GET", "/admin/feedback/reports", Some(&kim), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, "PUT", "/admin/feedback/readers/kim", Some(&kim), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, view) = call(
        &app,
        "PUT",
        "/admin/feedback/readers/kim",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["readers"][0]["username"], "kim");
    let (status, _) = call(
        &app,
        "PUT",
        "/admin/feedback/readers/nobody",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, access) = call(&app, "GET", "/admin/feedback/access", Some(&kim), None).await;
    assert_eq!(access, json!({ "inbox": true, "admin": false }));

    // the list: spam apart, newest first, filters, pages
    let (status, page) = call(&app, "GET", "/admin/feedback/reports", Some(&kim), None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["reports"].as_array().unwrap().len(), 2);
    assert_eq!(
        page["counts"],
        json!({ "new": 2, "open": 0, "done": 0, "spam": 1 })
    );
    assert_eq!(
        page["reports"][0]["excerpt"],
        "Export to PDF loses the images."
    );
    assert_eq!(page["platforms"], json!(["windows", "android"]));
    let get = |path: String| {
        let (app, kim) = (app.clone(), kim.clone());
        async move { call(&app, "GET", &path, Some(&kim), None).await }
    };
    let (_, android) = get("/admin/feedback/reports?platform=android".into()).await;
    assert_eq!(android["reports"].as_array().unwrap().len(), 1);
    let (_, spam) = get("/admin/feedback/reports?status=spam".into()).await;
    assert_eq!(spam["reports"][0]["flags"], "links");
    let (_, one) = get("/admin/feedback/reports?limit=1".into()).await;
    let next = one["next"].as_str().unwrap().to_owned();
    let (_, rest) = get(format!("/admin/feedback/reports?limit=1&before={next}")).await;
    assert_eq!(
        rest["reports"][0]["excerpt"],
        "The sidebar does not open on Android."
    );
    assert!(rest.get("next").is_none());

    // one report whole, its image as a download
    let id = rest["reports"][0]["id"].as_str().unwrap().to_owned();
    let (_, full) = get(format!("/admin/feedback/reports/{id}")).await;
    assert_eq!(full["images"][0]["width"], 8);
    assert_eq!(full["install"], install);
    let req = Request::get(format!("/api/v1/admin/feedback/reports/{id}/images/0"))
        .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
        .header("authorization", format!("Bearer {kim}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    assert_eq!(headers["content-type"], "image/png");
    assert!(
        headers["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    assert_eq!(headers["x-content-type-options"], "nosniff");
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    assert_eq!(bytes.to_vec(), png);

    // marked, noted, kept
    let path = format!("/admin/feedback/reports/{id}");
    let change = json!({ "status": "done", "note": "fixed in 0.12.1", "kept": true });
    let (status, changed) = call(&app, "PATCH", &path, Some(&kim), Some(change)).await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(
        (&changed["status"], &changed["note"], &changed["kept"]),
        (&json!("done"), &json!("fixed in 0.12.1"), &json!(true))
    );
    let open = Some(json!({ "status": "open" }));
    let (status, _) = call(
        &app,
        "PATCH",
        "/admin/feedback/reports/nope",
        Some(&kim),
        open,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // blocking its install: its next report is dropped
    let install_block = Some(json!({ "kind": "install" }));
    let (status, blocked) = call(
        &app,
        "POST",
        &format!("/admin/feedback/reports/{id}/block"),
        Some(&kim),
        install_block.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{blocked}");
    assert_eq!(blocked["status"], "spam");
    let other = page["reports"][0]["id"].as_str().unwrap().to_owned();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/admin/feedback/reports/{other}/block"),
        Some(&kim),
        install_block,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "that one has no install id"
    );
    let again = json!({ "install": install, "message": "a report from a blocked install" });
    assert_eq!(
        report(&app, "10.0.0.5", again).await.0,
        StatusCode::NO_CONTENT
    );
    let (_, after) = get("/admin/feedback/reports?status=new".into()).await;
    assert_eq!(after["counts"]["new"], 1, "the blocked one is not kept");
    let word = Some(json!({ "kind": "word", "value": "Casino", "days": 30 }));
    let (_, list) = call(&app, "POST", "/admin/feedback/blocks", Some(&admin), word).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["value"], "casino");
    let unblock = "/admin/feedback/blocks?kind=word&value=casino";
    let (_, list) = call(&app, "DELETE", unblock, Some(&admin), None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    // the read-only token for reports pull
    let (status, _) = call(&app, "POST", "/admin/feedback/token", Some(&kim), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "readers make no tokens");
    let (_, made) = call(&app, "POST", "/admin/feedback/token", Some(&admin), None).await;
    let pull = made["token"].as_str().unwrap().to_owned();
    assert!(pull.starts_with("fb_"));
    let since = "/admin/feedback/reports?after=0&status=new";
    let (status, since) = call(&app, "GET", since, Some(&pull), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        since["reports"][0]["excerpt"],
        "Export to PDF loses the images."
    );
    let (status, _) = call(&app, "GET", &path, Some(&pull), None).await;
    assert_eq!(status, StatusCode::OK);
    for (method, path, body) in [
        ("PATCH", path.clone(), Some(json!({ "status": "open" }))),
        ("DELETE", path.clone(), None),
        ("GET", "/admin/feedback/blocks".to_owned(), None),
        ("GET", "/admin/feedback/settings".to_owned(), None),
        ("GET", "/me".to_owned(), None),
    ] {
        let (status, _) = call(&app, method, &path, Some(&pull), body).await;
        assert!(
            matches!(status, StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED),
            "{method} {path}: {status}"
        );
    }
    let (_, view) = call(&app, "GET", "/admin/feedback/settings", Some(&admin), None).await;
    assert!(view["tokenCreatedAt"].is_number());
    call(&app, "DELETE", "/admin/feedback/token", Some(&admin), None).await;
    let (status, _) = call(&app, "GET", "/admin/feedback/reports", Some(&pull), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // deleting, then kim reads no more
    let (status, _) = call(&app, "DELETE", &path, Some(&kim), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let kim_id = view["readers"][0]["id"].as_str().unwrap().to_owned();
    let reader = format!("/admin/feedback/readers/{kim_id}");
    call(&app, "DELETE", &reader, Some(&admin), None).await;
    let (status, _) = call(&app, "GET", "/admin/feedback/reports", Some(&kim), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let log = events(&state).await;
    for event in [
        "feedback_reader_added",
        "feedback_blocked",
        "feedback_token",
        "feedback_deleted",
        "feedback_reader_removed",
    ] {
        assert!(log.iter().any(|e| e == event), "{event}");
    }
}

#[tokio::test]
async fn a_sender_reads_how_their_report_stands() {
    use sha2::{Digest, Sha256};
    let (_, app, admin) = receiving(FeedbackConfig::default()).await;
    let secret = "a-secret-only-the-app-keeps";
    let receipt = Sha256::digest(secret.as_bytes())
        .iter()
        .fold(String::new(), |hex, b| hex + &format!("{b:02x}"));
    let ask = |secrets: Value| {
        let app = app.clone();
        async move {
            let body = serde_json::to_vec(&json!({ "secrets": secrets })).unwrap();
            from(
                &app,
                "198.51.100.9",
                "POST",
                "/feedback/answers",
                Some(body),
            )
            .await
        }
    };

    // a receipt that is not a hash is refused
    let (status, _, _) = report(&app, "198.51.100.7", json!({ "receipt": "nope" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, body) = report(&app, "198.51.100.7", json!({ "receipt": receipt })).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, _, answers) = ask(json!([secret, "unknown"])).await;
    assert_eq!(status, StatusCode::OK, "{answers}");
    assert_eq!(
        answers,
        json!({ "reports": [{ "secret": secret, "state": "received", "answer": "" }] })
    );

    let (_, page) = call(&app, "GET", "/admin/feedback/reports", Some(&admin), None).await;
    let id = page["reports"][0]["id"].as_str().unwrap();
    let path = format!("/admin/feedback/reports/{id}");
    let (_, full) = call(&app, "GET", &path, Some(&admin), None).await;
    assert_eq!(full["answerable"], true);
    let change = json!({ "status": "done", "answer": "Fixed in 0.2.1, thank you" });
    let (status, changed) = call(&app, "PATCH", &path, Some(&admin), Some(change)).await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["answer"], "Fixed in 0.2.1, thank you");

    let (_, _, answers) = ask(json!([secret])).await;
    assert_eq!(answers["reports"][0]["state"], "done");
    assert_eq!(answers["reports"][0]["answer"], "Fixed in 0.2.1, thank you");

    // too many at once, or empty
    let many: Vec<String> = (0..=app_protocol::feedback::MAX_ASKED)
        .map(|i| i.to_string())
        .collect();
    let (status, _, _) = ask(json!(many)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = ask(json!([""])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
