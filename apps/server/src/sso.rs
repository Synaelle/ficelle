// SPDX-License-Identifier: AGPL-3.0-or-later

//! Single sign-on with an OpenID Connect provider: the authorization code
//! flow with PKCE, a state and a nonce. The provider's discovery document
//! is fetched on first use and kept for an hour, so the server starts even
//! while the provider is down.

use std::time::{Duration, Instant};

use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, reqwest,
};
use tokio::sync::Mutex;

use crate::config::Config;

/// How long discovery results are kept.
const DISCOVERY_LIFE: Duration = Duration::from_secs(3600);
/// Where the provider sends people back, under `server.public_url`.
pub const CALLBACK_PATH: &str = "/sso/callback";

/// The provider, as configured.
pub struct Provider {
    issuer: String,
    client_id: String,
    client_secret: Option<String>,
    scopes: Vec<String>,
    redirect: String,
    http: reqwest::Client,
    metadata: Mutex<Option<(CoreProviderMetadata, Instant)>>,
}

/// What the sign-in leaves for the callback.
#[derive(Debug, Clone)]
pub struct Started {
    /// Where to send the browser.
    pub url: String,
    /// Proves the callback comes from the browser that started.
    pub verifier: String,
    /// Ties the id token to this sign-in.
    pub nonce: String,
}

/// Who the provider says signed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The provider, its issuer address.
    pub issuer: String,
    /// The provider's stable id for the person.
    pub subject: String,
    /// The provider's username for them, if it gave one.
    pub username: Option<String>,
    /// Their email, if it gave one.
    pub email: Option<String>,
    /// Their name, if it gave one.
    pub name: Option<String>,
}

impl Identity {
    /// What to show for this identity: the email, else a username, else the id.
    pub fn label(&self) -> String {
        self.email
            .clone()
            .or_else(|| self.username.clone())
            .unwrap_or_else(|| self.subject.clone())
    }
}

impl Provider {
    /// The provider from `[sso]`, `None` when single sign-on is off.
    pub fn new(config: &Config) -> Option<Self> {
        let sso = &config.sso;
        if !sso.enabled() {
            return None;
        }
        // the provider's answers never redirect anywhere: a redirect could
        // point the server at an address inside its own network
        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        Some(Self {
            issuer: sso.issuer.clone(),
            client_id: sso.client_id.clone(),
            client_secret: Some(sso.client_secret.clone()).filter(|s| !s.is_empty()),
            scopes: sso.scopes.clone(),
            redirect: format!(
                "{}{CALLBACK_PATH}",
                config.server.public_url.trim_end_matches('/')
            ),
            http,
            metadata: Mutex::new(None),
        })
    }

    async fn metadata(&self) -> Result<CoreProviderMetadata, String> {
        let mut cached = self.metadata.lock().await;
        if let Some((metadata, at)) = cached.as_ref()
            && at.elapsed() < DISCOVERY_LIFE
        {
            return Ok(metadata.clone());
        }
        let issuer = IssuerUrl::new(self.issuer.clone()).map_err(|e| e.to_string())?;
        let metadata = CoreProviderMetadata::discover_async(issuer, &self.http)
            .await
            .map_err(|e| format!("discovery at {}: {e}", self.issuer))?;
        *cached = Some((metadata.clone(), Instant::now()));
        Ok(metadata)
    }

    /// The provider's sign-in address, carrying `state`.
    ///
    /// # Errors
    ///
    /// When the provider cannot be reached or its discovery document is wrong.
    pub async fn start(&self, state: &str) -> Result<Started, String> {
        let metadata = self.metadata().await?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.client_id.clone()),
            self.client_secret.clone().map(ClientSecret::new),
        )
        .set_redirect_uri(RedirectUrl::new(self.redirect.clone()).map_err(|e| e.to_string())?);
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let state = state.to_owned();
        let mut request = client.authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            move || CsrfToken::new(state),
            Nonce::new_random,
        );
        for scope in &self.scopes {
            request = request.add_scope(Scope::new(scope.clone()));
        }
        let (url, _, nonce) = request.set_pkce_challenge(challenge).url();
        Ok(Started {
            url: url.into(),
            verifier: verifier.secret().clone(),
            nonce: nonce.secret().clone(),
        })
    }

    /// Trades the code from the callback for a checked id token.
    ///
    /// # Errors
    ///
    /// When the provider refuses, cannot be reached, or the token does not
    /// check out: wrong signature, issuer, audience, nonce or expiry.
    pub async fn finish(
        &self,
        code: &str,
        verifier: &str,
        nonce: &str,
    ) -> Result<Identity, String> {
        let metadata = self.metadata().await?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.client_id.clone()),
            self.client_secret.clone().map(ClientSecret::new),
        )
        .set_redirect_uri(RedirectUrl::new(self.redirect.clone()).map_err(|e| e.to_string())?);
        let response = client
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .map_err(|e| e.to_string())?
            .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_owned()))
            .request_async(&self.http)
            .await
            .map_err(|e| format!("token request: {e}"))?;
        let token = response
            .id_token()
            .ok_or_else(|| "the provider sent no id token".to_owned())?;
        let claims = token
            .claims(&client.id_token_verifier(), &Nonce::new(nonce.to_owned()))
            .map_err(|e| format!("id token: {e}"))?;
        Ok(Identity {
            issuer: claims.issuer().to_string(),
            subject: claims.subject().to_string(),
            username: claims.preferred_username().map(|u| u.to_string()),
            email: claims.email().map(|e| e.to_string()),
            name: claims
                .name()
                .and_then(|n| n.get(None))
                .map(|n| n.to_string()),
        })
    }
}
