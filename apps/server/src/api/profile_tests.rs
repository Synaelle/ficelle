// SPDX-License-Identifier: AGPL-3.0-or-later

//! Email, password reset, export, deletion and the privacy facts, through
//! the full router with mail kept in memory.

use axum::Router;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::tests::{admin_on, call, events, login, server_with, with_admin};
use crate::clock::DAY;
use crate::config::Config;
use crate::mail::TEST_HOST;
use crate::routes::AppState;

const PASSWORD: &str = "admin password";

async fn with_mail() -> (AppState, Router, String) {
    let mut config = Config::default();
    config.server.public_url = "https://notes.example.org".into();
    config.smtp.host = TEST_HOST.into();
    config.smtp.from = "Notes <notes@example.org>".into();
    config.privacy.operator = "Le club".into();
    admin_on(server_with(config).await).await
}

/// The path of the link in the last mail, after the server address.
fn last_link(state: &AppState) -> String {
    let sent = state.mailer.sent();
    let body = &sent.last().expect("a mail was sent").body;
    let start = body
        .find("https://notes.example.org/")
        .expect("a link in the mail");
    body[start + "https://notes.example.org".len()..]
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

async fn confirmed_email(state: &AppState, app: &Router, token: &str) {
    let (status, _) = call(
        app,
        "PUT",
        "/me/email",
        Some(token),
        Some(json!({ "email": "Admin@Example.org", "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let link = last_link(state);
    let verify = link.strip_prefix("/verify/").unwrap();
    let (status, body) = call(
        app,
        "POST",
        "/auth/email/verify",
        None,
        Some(json!({ "token": verify })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn an_address_is_kept_once_its_link_is_opened() {
    let (state, app, token) = with_mail().await;
    let (status, body) = call(
        &app,
        "PUT",
        "/me/email",
        Some(&token),
        Some(json!({ "email": "admin@example.org" })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("confirmNeeded"))
    );
    let (status, _) = call(
        &app,
        "PUT",
        "/me/email",
        Some(&token),
        Some(json!({ "email": "not an address", "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = call(
        &app,
        "PUT",
        "/me/email",
        Some(&token),
        Some(json!({ "email": "Admin@Example.ORG", "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let sent = state.mailer.sent();
    assert_eq!(sent[0].to, "Admin@example.org");
    assert_eq!(sent[0].subject, "Confirm your email address");
    assert!(sent[0].body.starts_with("Hello admin,"), "{}", sent[0].body);
    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    assert!(security.get("email").is_none(), "not before the link");
    assert_eq!(security["emailAvailable"], true);

    let link = last_link(&state);
    let verify = link.strip_prefix("/verify/").unwrap();
    let (status, body) = call(
        &app,
        "POST",
        "/auth/email/verify",
        None,
        Some(json!({ "token": verify })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], "Admin@example.org");
    let (status, body) = call(
        &app,
        "POST",
        "/auth/email/verify",
        None,
        Some(json!({ "token": verify })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("linkInvalid")),
        "a link works once"
    );
    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    assert_eq!(security["email"], "Admin@example.org");
    assert!(events(&state).await.iter().any(|e| e == "email_confirmed"));
}

#[tokio::test]
async fn without_smtp_there_is_no_email_and_no_reset() {
    let (_, app, token) = with_admin().await;
    let (_, options) = call(&app, "GET", "/auth/options", None, None).await;
    assert_eq!(options["emailReset"], false);
    let (status, body) = call(
        &app,
        "PUT",
        "/me/email",
        Some(&token),
        Some(json!({ "email": "a@example.org", "password": PASSWORD })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("mailOff"))
    );
    let (status, _) = call(
        &app,
        "POST",
        "/auth/reset/request",
        None,
        Some(json!({ "username": "admin" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_reset_link_sets_a_new_password_and_signs_everyone_out() {
    let (state, app, token) = with_mail().await;
    let reset = |name: &'static str| {
        let app = app.clone();
        async move {
            call(
                &app,
                "POST",
                "/auth/reset/request",
                None,
                Some(json!({ "username": name })),
            )
            .await
            .0
        }
    };
    // no address yet, and nobody called that: the same answer, no mail
    assert_eq!(reset("admin").await, StatusCode::ACCEPTED);
    assert_eq!(reset("ghost").await, StatusCode::ACCEPTED);
    assert!(state.mailer.sent().is_empty());

    confirmed_email(&state, &app, &token).await;
    assert_eq!(reset("ADMIN").await, StatusCode::ACCEPTED);
    let link = last_link(&state);
    let reset_token = link.strip_prefix("/reset/").unwrap().to_owned();
    assert_eq!(
        state.mailer.sent().last().unwrap().subject,
        "Set a new password"
    );

    let finish = |password: &'static str| {
        let app = app.clone();
        let reset_token = reset_token.clone();
        async move {
            call(
                &app,
                "POST",
                "/auth/reset/finish",
                None,
                Some(json!({ "token": reset_token, "password": password })),
            )
            .await
        }
    };
    let (status, body) = finish("admin").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("passwordWeak"))
    );
    let (status, _) = finish("brand new password").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = finish("another new password").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "the link works once");

    let (status, _) = call(&app, "GET", "/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "every device signed out");
    assert_eq!(
        login(&app, "admin", PASSWORD).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, "admin", "brand new password").await.0,
        StatusCode::OK
    );
    let sent = state.mailer.sent();
    assert_eq!(sent.last().unwrap().subject, "Your password was changed");
    assert!(
        events(&state)
            .await
            .iter()
            .any(|e| e == "password_reset_by_email")
    );
}

#[tokio::test]
async fn an_expired_reset_link_does_nothing() {
    let (state, app, token) = with_mail().await;
    confirmed_email(&state, &app, &token).await;
    call(
        &app,
        "POST",
        "/auth/reset/request",
        None,
        Some(json!({ "username": "admin" })),
    )
    .await;
    let link = last_link(&state);
    state.clock.advance(2 * 60 * 60 * 1000);
    let (status, _) = call(
        &app,
        "POST",
        "/auth/reset/finish",
        None,
        Some(json!({ "token": link.strip_prefix("/reset/").unwrap(), "password": "brand new password" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(login(&app, "admin", PASSWORD).await.0, StatusCode::OK);
}

#[tokio::test]
async fn the_export_holds_the_account_but_no_secret() {
    let (state, app, token) = with_mail().await;
    confirmed_email(&state, &app, &token).await;
    let (status, export) = call(&app, "GET", "/me/export", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(export["account"]["username"], "admin");
    assert_eq!(export["account"]["email"], "Admin@example.org");
    assert_eq!(export["devices"].as_array().unwrap().len(), 1);
    assert!(
        export["securityLog"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event"] == "setup_done")
    );
    let text = export.to_string();
    assert!(!text.contains("argon2"), "no password hash");
    assert!(!text.contains("passwordHash") && !text.contains("password_hash"));
}

#[tokio::test]
async fn deletion_waits_for_the_grace_period_and_can_be_cancelled() {
    let (state, app, admin) = with_mail().await;
    // someone to delete, signed in on two devices
    call(
        &app,
        "POST",
        "/admin/users",
        Some(&admin),
        Some(json!({ "username": "zoe", "password": "zoe password" })),
    )
    .await;
    let (_, first) = login(&app, "zoe", "zoe password").await;
    let (_, second) = login(&app, "zoe", "zoe password").await;
    let token = first["accessToken"].as_str().unwrap().to_owned();
    let other = second["accessToken"].as_str().unwrap().to_owned();

    let (status, _) = call(&app, "POST", "/me/deletion", Some(&token), Some(json!({}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "needs the password");
    let (status, body) = call(
        &app,
        "POST",
        "/me/deletion",
        Some(&token),
        Some(json!({ "password": "zoe password" })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let at = body["deleteAt"].as_i64().unwrap();
    assert!(at - state.clock.now() > 29 * DAY);
    assert_eq!(
        call(&app, "GET", "/me", Some(&other), None).await.0,
        StatusCode::UNAUTHORIZED,
        "other devices signed out"
    );
    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    assert_eq!(security["deleteAt"], at);

    let (status, _) = call(&app, "DELETE", "/me/deletion", Some(&token), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    state.clock.advance(31 * DAY);
    assert_eq!(super::run_deletions(&state).await.unwrap(), 0);
    let (_, again) = login(&app, "zoe", "zoe password").await;
    let token = again["accessToken"].as_str().unwrap().to_owned();

    call(
        &app,
        "POST",
        "/me/deletion",
        Some(&token),
        Some(json!({ "password": "zoe password" })),
    )
    .await;
    state.clock.advance(29 * DAY);
    assert_eq!(super::run_deletions(&state).await.unwrap(), 0, "not yet");
    state.clock.advance(2 * DAY);
    assert_eq!(super::run_deletions(&state).await.unwrap(), 1);
    assert_eq!(
        login(&app, "zoe", "zoe password").await.0,
        StatusCode::UNAUTHORIZED
    );
    // the admin's own token is long expired by now
    let (_, admin) = login(&app, "admin", PASSWORD).await;
    let admin = admin["accessToken"].as_str().unwrap();
    let (_, log) = call(
        &app,
        "GET",
        "/admin/audit?event=user_deleted",
        Some(admin),
        None,
    )
    .await;
    let detail: Vec<&Value> = log["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["detail"])
        .collect();
    assert!(
        detail.iter().any(|d| d
            .as_str()
            .unwrap_or("")
            .contains("zoe, after the grace period")),
        "{log}"
    );
}

#[tokio::test]
async fn the_last_admin_cannot_ask_to_be_deleted() {
    let (_, app, token) = with_mail().await;
    let (status, body) = call(
        &app,
        "POST",
        "/me/deletion",
        Some(&token),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::CONFLICT, Some("lastAdmin"))
    );
}

#[tokio::test]
async fn privacy_facts_come_from_the_configuration() {
    let (_, app, _) = with_mail().await;
    let (status, info) = call(&app, "GET", "/privacy", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["operator"], "Le club");
    assert!(info.get("contact").is_none());
    assert_eq!(info["email"], true);
    assert_eq!(info["auditDays"], 365);
    assert_eq!(info["deletionGraceDays"], 30);
    assert_eq!(info["accountKinds"], "choice");
    // feedback: not taken here by default, people's reports go to the project
    assert_eq!(info["feedbackReceive"], false);
    assert_eq!(info["feedbackKeepDays"], 180);
    assert_eq!(info["feedbackSendTo"], "main");
    assert_eq!(
        info["feedbackMain"],
        app_brand::FEEDBACK_URL
            .split("://")
            .nth(1)
            .unwrap_or_default()
    );
}

/// The six digits of the last mail.
fn last_code(state: &AppState) -> String {
    let sent = state.mailer.sent();
    let body = &sent.last().expect("a mail was sent").body;
    body.split_whitespace()
        .find(|w| w.len() == 6 && w.bytes().all(|b| b.is_ascii_digit()))
        .expect("a code in the mail")
        .to_owned()
}

#[tokio::test]
async fn a_code_by_email_as_the_second_step() {
    let (state, app, token) = with_mail().await;
    // not without a confirmed address
    let (status, _) = call(&app, "POST", "/me/email-codes/enable", Some(&token), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    confirmed_email(&state, &app, &token).await;
    let (status, _) = call(&app, "POST", "/me/email-codes/enable", Some(&token), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    assert_eq!(security["emailCodes"], true);

    // the password asks for the code next
    let (status, needed) = login(&app, "admin", PASSWORD).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{needed}");
    assert_eq!(needed["methods"], json!(["email"]));
    let challenge = needed["challenge"].clone();
    let (status, _) = call(
        &app,
        "POST",
        "/auth/second-factor/email",
        None,
        Some(json!({ "challenge": challenge })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let code = last_code(&state);
    let answer = |code: &str| json!({ "challenge": challenge, "method": "email", "code": code });
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let (status, _) = call(
        &app,
        "POST",
        "/auth/second-factor",
        None,
        Some(answer(wrong)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, session) = call(
        &app,
        "POST",
        "/auth/second-factor",
        None,
        Some(answer(&code)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");

    // a few codes per sign-in at most
    let (_, needed) = login(&app, "admin", PASSWORD).await;
    let ask = json!({ "challenge": needed["challenge"] });
    for _ in 0..crate::accounts::factors::EMAIL_SENDS {
        let (status, _) = call(
            &app,
            "POST",
            "/auth/second-factor/email",
            None,
            Some(ask.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let (status, _) = call(&app, "POST", "/auth/second-factor/email", None, Some(ask)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // off again with the password, then the password alone signs in
    let (status, _) = call(
        &app,
        "POST",
        "/me/email-codes/disable",
        Some(&token),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = login(&app, "admin", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
}
