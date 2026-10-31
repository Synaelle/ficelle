// SPDX-License-Identifier: AGPL-3.0-or-later

//! What handlers take from a request: the client's address, the signed in
//! account, and a json body whose mistakes come back in the api's error shape.

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use serde::de::DeserializeOwned;

use crate::accounts::store::{self, Authed};
use crate::accounts::token;
use crate::config::ServerConfig;
use crate::error::Error;
use crate::mail::Lang;
use crate::routes::AppState;

/// The client's address. Behind a trusted proxy it is the last address in
/// `X-Forwarded-For` that is not itself a trusted proxy: earlier entries are
/// whatever the client claimed and cannot be believed.
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl ClientIp {
    fn of(parts: &Parts, server: &ServerConfig) -> Self {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip());
        if !server.trusts(peer) {
            return Self(peer);
        }
        let forwarded: Vec<IpAddr> = parts
            .headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .filter_map(|ip| ip.trim().parse().ok())
            .collect();
        let real = forwarded
            .iter()
            .rev()
            .find(|ip| !server.trusts(**ip))
            .copied();
        Self(real.unwrap_or(peer))
    }
}

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(Ok(Self::of(parts, &state.config.server)))
    }
}

/// The address the connection comes from, a proxy or the client itself.
#[derive(Debug, Clone, Copy)]
pub struct Peer(pub IpAddr);

impl FromRequestParts<AppState> for Peer {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _: &AppState,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip()),
        )))
    }
}

/// Whether the request may use setup, see [`crate::setup::secure`].
#[derive(Debug, Clone, Copy)]
pub struct Secure(pub bool);

impl FromRequestParts<AppState> for Secure {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip());
        let client = ClientIp::of(parts, &state.config.server).0;
        std::future::ready(Ok(Self(crate::setup::secure(
            &parts.headers,
            peer,
            client,
            &state.config,
        ))))
    }
}

/// The language of the request, for the mails it causes.
pub struct Language(pub Lang);

impl<S: Send + Sync> FromRequestParts<S> for Language {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let header = parts
            .headers
            .get(header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok());
        std::future::ready(Ok(Self(Lang::from_header(header))))
    }
}

/// The signed in account behind `Authorization: Bearer <access token>`.
pub struct CurrentUser(pub Authed);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let bearer = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(Error::unauthorized)?;
        let hash = token::hash(bearer.trim());
        let now = state.clock.now();
        let authed = state
            .db
            .call(move |c| store::authenticate(c, &hash, now))
            .await?;
        authed.map(Self).ok_or_else(Error::unauthorized)
    }
}

/// A json body. A malformed one is a 400 `invalid` naming the problem.
pub struct Body<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for Body<T> {
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            // too large stays a 413, the limit is not a client's typo
            Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                Err(Error::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    app_protocol::error_code::INVALID,
                    rejection.body_text(),
                ))
            }
            Err(rejection) => Err(Error::invalid(match rejection {
                JsonRejection::JsonDataError(e) => e.body_text(),
                other => other.body_text(),
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Db;

    async fn ip_of(peer: &str, forwarded: Option<&str>) -> IpAddr {
        let state = AppState::new(Config::default(), Db::in_memory().unwrap());
        let mut req = axum::http::Request::builder().uri("/");
        if let Some(f) = forwarded {
            req = req.header("x-forwarded-for", f);
        }
        let (mut parts, ()) = req.body(()).unwrap().into_parts();
        parts
            .extensions
            .insert(ConnectInfo(SocketAddr::new(peer.parse().unwrap(), 50_000)));
        let Ok(ClientIp(ip)) = ClientIp::from_request_parts(&mut parts, &state).await;
        ip
    }

    #[tokio::test]
    async fn the_real_address_comes_through_trusted_proxies_only() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert_eq!(
            ip_of("203.0.113.7", Some("1.2.3.4")).await,
            ip("203.0.113.7"),
            "an untrusted peer cannot claim an address"
        );
        assert_eq!(
            ip_of("127.0.0.1", Some("203.0.113.7")).await,
            ip("203.0.113.7")
        );
        assert_eq!(
            ip_of("127.0.0.1", Some("6.6.6.6, 203.0.113.7")).await,
            ip("203.0.113.7"),
            "the client's own claim on the left is ignored"
        );
        assert_eq!(ip_of("127.0.0.1", None).await, ip("127.0.0.1"));
    }
}
