// SPDX-License-Identifier: AGPL-3.0-or-later

//! Single sign-on against a small provider running in the test: discovery,
//! keys and a token endpoint that signs real id tokens.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Form, State};
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use openidconnect::core::{
    CoreIdToken, CoreIdTokenClaims, CoreIdTokenFields, CoreJsonWebKeySet, CoreJwsSigningAlgorithm,
    CoreProviderMetadata, CoreResponseType, CoreRsaPrivateSigningKey, CoreSubjectIdentifierType,
    CoreTokenResponse, CoreTokenType,
};
use openidconnect::{
    AccessToken, Audience, AuthUrl, EmptyAdditionalClaims, EmptyAdditionalProviderMetadata,
    EmptyExtraTokenFields, EndUserEmail, EndUserUsername, IssuerUrl, JsonWebKeyId,
    JsonWebKeySetUrl, Nonce, PrivateSigningKey, ResponseTypes, StandardClaims, SubjectIdentifier,
    TokenUrl,
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::tests::{admin_on, call, device, events, server_with};
use crate::config::Config;
use crate::routes::AppState;

const CLIENT: &str = "notes";
const PASSWORD: &str = "admin password";

/// Who the provider will say signed in, by authorization code.
type Logins = Arc<Mutex<HashMap<String, (String, String, String)>>>;

struct Idp {
    issuer: String,
    logins: Logins,
}

#[derive(Clone)]
struct IdpState {
    issuer: String,
    key: Arc<CoreRsaPrivateSigningKey>,
    logins: Logins,
}

async fn provider() -> Idp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let pem = openssl::rsa::Rsa::generate(2048)
        .unwrap()
        .private_key_to_pem()
        .unwrap();
    let key = CoreRsaPrivateSigningKey::from_pem(
        std::str::from_utf8(&pem).unwrap(),
        Some(JsonWebKeyId::new("k1".into())),
    )
    .unwrap();
    let logins = Logins::default();
    let state = IdpState {
        issuer: issuer.clone(),
        key: Arc::new(key),
        logins: logins.clone(),
    };
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks", get(jwks))
        .route("/token", post(token))
        .with_state(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Idp { issuer, logins }
}

async fn discovery(State(idp): State<IdpState>) -> axum::Json<CoreProviderMetadata> {
    let at = |path: &str| format!("{}{path}", idp.issuer);
    axum::Json(
        CoreProviderMetadata::new(
            IssuerUrl::new(idp.issuer.clone()).unwrap(),
            AuthUrl::new(at("/authorize")).unwrap(),
            JsonWebKeySetUrl::new(at("/jwks")).unwrap(),
            vec![ResponseTypes::new(vec![CoreResponseType::Code])],
            vec![CoreSubjectIdentifierType::Public],
            vec![CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256],
            EmptyAdditionalProviderMetadata {},
        )
        .set_token_endpoint(Some(TokenUrl::new(at("/token")).unwrap())),
    )
}

async fn jwks(State(idp): State<IdpState>) -> axum::Json<CoreJsonWebKeySet> {
    axum::Json(CoreJsonWebKeySet::new(vec![idp.key.as_verification_key()]))
}

async fn token(
    State(idp): State<IdpState>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<axum::Json<CoreTokenResponse>, StatusCode> {
    // the server must prove it started the sign-in
    if form.get("code_verifier").is_none_or(String::is_empty) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let code = form.get("code").ok_or(StatusCode::BAD_REQUEST)?;
    let (nonce, subject, username) = idp
        .logins
        .lock()
        .unwrap()
        .remove(code)
        .ok_or(StatusCode::BAD_REQUEST)?;
    let now = chrono::Utc::now();
    let claims = CoreIdTokenClaims::new(
        IssuerUrl::new(idp.issuer.clone()).unwrap(),
        vec![Audience::new(CLIENT.into())],
        now + chrono::Duration::minutes(5),
        now,
        StandardClaims::new(SubjectIdentifier::new(subject))
            .set_preferred_username(Some(EndUserUsername::new(username.clone())))
            .set_email(Some(EndUserEmail::new(format!("{username}@example.org")))),
        EmptyAdditionalClaims {},
    )
    .set_nonce(Some(Nonce::new(nonce)));
    let id_token = CoreIdToken::new(
        claims,
        idp.key.as_ref(),
        CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
        None,
        None,
    )
    .unwrap();
    Ok(axum::Json(CoreTokenResponse::new(
        AccessToken::new("provider access token".into()),
        CoreTokenType::Bearer,
        CoreIdTokenFields::new(Some(id_token), EmptyExtraTokenFields {}),
    )))
}

async fn with_sso(create_accounts: bool) -> (AppState, Router, String, Idp) {
    let idp = provider().await;
    let mut config = Config::default();
    config.server.public_url = "https://notes.example.org".into();
    config.sso.issuer = idp.issuer.clone();
    config.sso.client_id = CLIENT.into();
    config.sso.client_secret = "secret".into();
    config.sso.label = "Company account".into();
    config.sso.create_accounts = create_accounts;
    let (state, app, token) = admin_on(server_with(config).await).await;
    (state, app, token, idp)
}

/// Starts a sign-on as a browser would: the answer and the binding cookie
/// it set (`name=value`).
async fn start(
    app: &Router,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value, String) {
    let mut req = Request::post(format!("/api/v1{path}"))
        .header(
            app_protocol::PROTOCOL_HEADER,
            app_protocol::PROTOCOL_VERSION.to_string(),
        )
        .header("content-type", "application/json");
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let bytes = to_bytes(response.into_body(), 1 << 16).await.unwrap();
    let answer = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let pair = cookie.split(';').next().unwrap_or("").to_owned();
    (status, answer, pair)
}

/// Goes through the provider as `subject`, back in the browser holding
/// `cookie`: returns where the callback leads.
async fn through_provider(
    app: &Router,
    idp: &Idp,
    url: &str,
    cookie: &str,
    subject: &str,
    username: &str,
) -> String {
    let url = url::Url::parse(url).unwrap();
    assert!(
        url.as_str()
            .starts_with(&format!("{}/authorize", idp.issuer))
    );
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(
        query["redirect_uri"],
        "https://notes.example.org/sso/callback"
    );
    let code = format!("code-{subject}");
    idp.logins.lock().unwrap().insert(
        code.clone(),
        (query["nonce"].clone(), subject.into(), username.into()),
    );
    let callback = format!("/sso/callback?code={code}&state={}", query["state"]);
    let response = app
        .clone()
        .oneshot(
            Request::get(callback)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    let _ = to_bytes(response.into_body(), 1 << 16).await;
    location
}

async fn sign_in(
    app: &Router,
    idp: &Idp,
    subject: &str,
    username: &str,
) -> (String, (StatusCode, Value)) {
    let (status, started, cookie) = start(
        app,
        "/auth/sso/start",
        None,
        json!({ "returnTo": "/admin" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let location = through_provider(
        app,
        idp,
        started["url"].as_str().unwrap(),
        &cookie,
        subject,
        username,
    )
    .await;
    let Some(code) = location.strip_prefix("/admin#sso=") else {
        return (location, (StatusCode::UNAUTHORIZED, Value::Null));
    };
    let exchanged = exchange(app, code, &cookie).await;
    (location, exchanged)
}

/// The code exchanged in the browser holding `cookie`.
async fn exchange(app: &Router, code: &str, cookie: &str) -> (StatusCode, Value) {
    let body = json!({ "code": code, "device": device("browser") });
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/sso/exchange")
                .header(
                    app_protocol::PROTOCOL_HEADER,
                    app_protocol::PROTOCOL_VERSION.to_string(),
                )
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 16).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A code signs in only the browser the
/// sign-in went through, not someone sent a link with it.
#[tokio::test]
async fn a_code_signs_in_only_the_browser_that_signed_in() {
    let (_, app, _, idp) = with_sso(true).await;
    let (status, started, cookie) = start(
        &app,
        "/auth/sso/start",
        None,
        json!({ "returnTo": "/admin" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let location = through_provider(
        &app,
        &idp,
        started["url"].as_str().unwrap(),
        &cookie,
        "s-1",
        "zoe",
    )
    .await;
    let code = location.strip_prefix("/admin#sso=").unwrap();
    let (status, _) = exchange(&app, code, "sso_binding=someone-else").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "another browser");
    let (_, again, _) = start(
        &app,
        "/auth/sso/start",
        None,
        json!({ "returnTo": "/admin" }),
    )
    .await;
    let location = through_provider(
        &app,
        &idp,
        again["url"].as_str().unwrap(),
        &cookie,
        "s-1",
        "zoe",
    )
    .await;
    assert!(
        location.contains("sso-error"),
        "a state from another start: {location}"
    );
}

#[tokio::test]
async fn without_an_issuer_sso_is_off() {
    let (_, app, _) = super::tests::with_admin().await;
    let (_, options) = call(&app, "GET", "/auth/options", None, None).await;
    assert!(options.get("sso").is_none());
    let (status, body) = call(
        &app,
        "POST",
        "/auth/sso/start",
        None,
        Some(json!({ "returnTo": "/admin" })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("ssoUnavailable"))
    );
}

#[tokio::test]
async fn an_unknown_identity_gets_no_account_unless_allowed() {
    let (_, app, _, idp) = with_sso(false).await;
    let (_, options) = call(&app, "GET", "/auth/options", None, None).await;
    assert_eq!(options["sso"], "Company account");
    let (location, _) = sign_in(&app, &idp, "s-1", "zoe").await;
    assert_eq!(location, "/admin#sso-error=ssoNoAccount");
}

#[tokio::test]
async fn a_first_sign_in_makes_an_account_without_a_password() {
    let (state, app, _, idp) = with_sso(true).await;
    let (_, (status, session)) = sign_in(&app, &idp, "s-1", "Zoe").await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["user"]["username"], "zoe");
    let token = session["accessToken"].as_str().unwrap();
    let (_, security) = call(&app, "GET", "/me/security", Some(token), None).await;
    assert_eq!(security["hasPassword"], false);
    assert_eq!(security["sso"][0]["name"], "Zoe@example.org");

    // the same identity signs into the same account, a namesake gets another
    let (_, (_, again)) = sign_in(&app, &idp, "s-1", "Zoe").await;
    assert_eq!(again["user"]["id"], session["user"]["id"]);
    let (_, (_, other)) = sign_in(&app, &idp, "s-2", "zoe").await;
    assert_eq!(other["user"]["username"], "zoe-2");

    // no password: the password form never opens the account
    let (status, _) = super::tests::login(&app, "zoe", "").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // and its only identity stays
    let id = security["sso"][0]["id"].as_str().unwrap();
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/me/sso/{id}"),
        Some(token),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(events(&state).await.iter().any(|e| e == "registered"));
}

#[tokio::test]
async fn a_code_works_once_and_a_state_belongs_to_one_sign_in() {
    let (_, app, _, idp) = with_sso(true).await;
    let (location, (status, _)) = sign_in(&app, &idp, "s-1", "zoe").await;
    assert_eq!(status, StatusCode::OK);
    let code = location.strip_prefix("/admin#sso=").unwrap();
    let (status, body) = call(
        &app,
        "POST",
        "/auth/sso/exchange",
        None,
        Some(json!({ "code": code, "device": device("browser") })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("ssoFailed"))
    );

    let forged = app
        .clone()
        .oneshot(
            Request::get("/sso/callback?code=x&state=ch_made_up")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forged.headers()["location"], "/admin#sso-error=ssoFailed");
}

#[tokio::test]
async fn only_paths_on_this_server_to_come_back_to() {
    let (_, app, _, _) = with_sso(true).await;
    for bad in ["https://evil.example", "//evil.example", "/\\evil", "admin"] {
        let (status, _) = call(
            &app,
            "POST",
            "/auth/sso/start",
            None,
            Some(json!({ "returnTo": bad })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn an_account_links_an_identity_then_signs_in_with_it() {
    let (state, app, token, idp) = with_sso(false).await;
    let (status, body) = call(
        &app,
        "POST",
        "/me/sso/link",
        Some(&token),
        Some(json!({ "returnTo": "/admin" })),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("confirmNeeded"))
    );
    let (status, started, cookie) = start(
        &app,
        "/me/sso/link",
        Some(&token),
        json!({ "returnTo": "/admin", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let location = through_provider(
        &app,
        &idp,
        started["url"].as_str().unwrap(),
        &cookie,
        "s-9",
        "boss",
    )
    .await;
    assert_eq!(location, "/admin#sso-linked");

    let (_, (status, session)) = sign_in(&app, &idp, "s-9", "boss").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["user"]["username"], "admin");
    let log = events(&state).await;
    assert!(log.iter().any(|e| e == "sso_linked"));

    // An admin with a second factor gives it
    // after the provider too
    let id = session["user"]["id"].as_str().unwrap().to_owned();
    let master = state.master.clone();
    state
        .db
        .call(move |c| {
            crate::accounts::factors::set_totp_pending(c, &master, &id, "JBSWY3DPEHPK3PXP")?;
            crate::accounts::factors::activate_totp(c, &id, 1)?;
            Ok(())
        })
        .await
        .unwrap();
    let (_, (status, asked)) = sign_in(&app, &idp, "s-9", "boss").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{asked}");
    assert!(asked["challenge"].is_string(), "{asked}");

    // an identity links once
    let (_, again, cookie) = start(
        &app,
        "/me/sso/link",
        Some(&token),
        json!({ "returnTo": "/admin", "password": PASSWORD }),
    )
    .await;
    let location = through_provider(
        &app,
        &idp,
        again["url"].as_str().unwrap(),
        &cookie,
        "s-9",
        "boss",
    )
    .await;
    assert_eq!(location, "/admin#sso-error=ssoTaken");
}

#[tokio::test]
async fn the_answer_counts_only_in_the_browser_that_started() {
    let (state, app, token, idp) = with_sso(true).await;
    // someone starts linking their account and sends the link to another
    // person, whose browser holds no cookie, or one of its own sign-ons
    let (_, linking, cookie) = start(
        &app,
        "/me/sso/link",
        Some(&token),
        json!({ "returnTo": "/admin", "password": PASSWORD }),
    )
    .await;
    assert!(cookie.starts_with("sso_binding=ch_"), "{cookie}");
    let url = linking["url"].as_str().unwrap();
    let location = through_provider(&app, &idp, url, "", "s-7", "victim").await;
    assert_eq!(location, "/admin#sso-error=ssoFailed", "no cookie");
    let (_, other, theirs) = start(
        &app,
        "/auth/sso/start",
        None,
        json!({ "returnTo": "/admin" }),
    )
    .await;
    let (_, again, _) = start(
        &app,
        "/me/sso/link",
        Some(&token),
        json!({ "returnTo": "/admin", "password": PASSWORD }),
    )
    .await;
    let location = through_provider(
        &app,
        &idp,
        again["url"].as_str().unwrap(),
        &theirs,
        "s-7",
        "victim",
    )
    .await;
    assert_eq!(
        location, "/admin#sso-error=ssoFailed",
        "another browser's cookie"
    );
    assert!(
        !events(&state).await.iter().any(|e| e == "sso_linked"),
        "nothing was linked"
    );
    // in its own browser it still works
    let location = through_provider(
        &app,
        &idp,
        other["url"].as_str().unwrap(),
        &theirs,
        "s-8",
        "sam",
    )
    .await;
    assert!(location.starts_with("/admin#sso="), "{location}");
}

#[tokio::test]
async fn the_binding_cookie_stays_with_the_callback() {
    let (_, app, _, _) = with_sso(true).await;
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/sso/start")
                .header(
                    app_protocol::PROTOCOL_HEADER,
                    app_protocol::PROTOCOL_VERSION.to_string(),
                )
                .header("content-type", "application/json")
                .body(Body::from(json!({ "returnTo": "/admin" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    for part in [
        "Path=/sso/callback",
        "HttpOnly",
        "SameSite=Lax",
        "Secure",
        "Max-Age=600",
    ] {
        assert!(cookie.contains(part), "{part} in {cookie}");
    }
}
