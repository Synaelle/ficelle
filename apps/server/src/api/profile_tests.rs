// SPDX-License-Identifier: AGPL-3.0-or-later

//! The export and the privacy facts, and a server that sent email before,
//! through the full router.

use axum::http::StatusCode;
use serde_json::json;

use super::tests::{admin_on, call, login, server_with, with_admin};
use crate::config::{AuthConfig, Config};
use crate::db::Db;
use crate::routes::{AppState, router};

const PASSWORD: &str = "admin password";

#[tokio::test]
async fn the_export_holds_the_account_but_no_secret() {
    let (_, app, token) = with_admin().await;
    let (status, export) = call(&app, "GET", "/me/export", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(export["account"]["username"], "admin");
    assert!(export["account"].get("email").is_none());
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
async fn privacy_facts_come_from_the_configuration() {
    let mut config = Config::default();
    config.privacy.operator = "Le club".into();
    let (_, app, _) = admin_on(server_with(config).await).await;
    let (status, info) = call(&app, "GET", "/privacy", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["operator"], "Le club");
    assert!(info.get("contact").is_none());
    assert_eq!(info["email"], false);
    assert_eq!(info["auditDays"], 365);
    assert!(info.get("accountKinds").is_none());
    // feedback: not taken here by default, people's reports go to the project
    assert_eq!(info["feedbackReceive"], false);
    assert_eq!(info["feedbackKeepDays"], 30);
    assert_eq!(info["feedbackSendTo"], "main");
    assert_eq!(
        info["feedbackMain"],
        app_brand::FEEDBACK_URL
            .split("://")
            .nth(1)
            .unwrap_or_default()
    );
}

#[tokio::test]
async fn nothing_about_email_answers_any_more() {
    let (_, app, token) = with_admin().await;
    let (_, options) = call(&app, "GET", "/auth/options", None, None).await;
    assert_eq!(options["emailReset"], false);
    let (_, security) = call(&app, "GET", "/me/security", Some(&token), None).await;
    assert_eq!(
        (&security["emailAvailable"], &security["emailCodes"]),
        (&json!(false), &json!(false))
    );
    for (method, path) in [
        ("PUT", "/me/email"),
        ("POST", "/auth/email/verify"),
        ("POST", "/auth/reset/request"),
        ("POST", "/auth/reset/finish"),
        ("POST", "/me/email-codes/enable"),
        ("POST", "/auth/second-factor/email"),
    ] {
        let (status, _) = call(&app, method, path, Some(&token), Some(json!({}))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }
}

#[tokio::test]
async fn an_admin_whose_second_step_was_codes_by_email_still_signs_in_after_the_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        auth: AuthConfig {
            argon2_memory_kib: 64,
            argon2_iterations: 1,
            ..AuthConfig::default()
        },
        ..Config::default()
    };
    // the hash as the older server made it, with the same settings
    let (made, _) = server_with(config.clone()).await;
    let hash = super::auth::hash_password(&made, PASSWORD.into())
        .await
        .unwrap();
    let old = Db::open_before(dir.path(), "DROP TABLE email_codes;").unwrap();
    old.call(move |c| {
        let user = crate::accounts::store::insert_user(
            c,
            "admin",
            "Admin",
            &hash,
            app_protocol::account::Role::Admin,
            1,
        )?
        .unwrap();
        c.execute(
            "UPDATE users SET email = 'admin@example.org', email_verified_at = 1,
                 email_codes = 1, factor_required = 0 WHERE id = ?1",
            [&user.id],
        )?;
        c.execute(
            "INSERT INTO recovery_codes (user_id, hash) VALUES (?1, x'01')",
            [&user.id],
        )?;
        c.execute(
            "INSERT INTO email_codes (challenge, code, expires_at) VALUES (x'02', x'03', 9)",
            [],
        )?;
        crate::setup::lock(c)?;
        Ok(())
    })
    .await
    .unwrap();
    drop(old);

    let state = AppState::for_tests(config, Db::open(dir.path()).unwrap());
    let app = router(state.clone());
    let (status, session) = login(&app, "admin", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "the password alone, {session}");
    assert_eq!(session["user"]["factorRequired"], true);
    let token = session["accessToken"].as_str().unwrap();
    let (status, body) = call(&app, "GET", "/admin/overview", Some(token), None).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("secondFactorRequired")),
        "a passkey or an authenticator app first"
    );
    let (_, security) = call(&app, "GET", "/me/security", Some(token), None).await;
    assert_eq!(security["recoveryCodesLeft"], 0);
    let columns: i64 = state
        .db
        .call(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM pragma_table_info('users') WHERE name LIKE 'email%'",
                [],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(columns, 0);
}
