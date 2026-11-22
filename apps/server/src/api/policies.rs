// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin's files policy for the instance, and the storage it uses. See
//! [`crate::files::policy`] for how it meets the config.

use app_protocol::admin::{PolicySettings, PolicyView, StorageOverview};
use axum::Json;
use axum::extract::State;

use super::admin::{admin, admin_event};
use super::extract::{Body, ClientIp, CurrentUser};
use crate::error::Error;
use crate::files::policy;
use crate::routes::AppState;

/// `GET /api/v1/admin/storage`
pub async fn storage(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<StorageOverview>, Error> {
    admin(&me)?;
    Ok(Json(state.db.call(|c| crate::files::storage(c)).await?))
}

async fn view(state: &AppState) -> Result<Json<PolicyView>, Error> {
    let config = state.config.clone();
    Ok(Json(
        state
            .db
            .call(move |c| {
                let settings = policy::settings(c)?;
                let (effective, sources) = policy::resolve(&config.files, &settings);
                Ok(PolicyView {
                    settings,
                    effective,
                    sources,
                })
            })
            .await?,
    ))
}

/// `GET /api/v1/admin/policy`
pub async fn instance(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<PolicyView>, Error> {
    admin(&me)?;
    view(&state).await
}

/// `PUT /api/v1/admin/policy`
pub async fn set_instance(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(settings): Body<PolicySettings>,
) -> Result<Json<PolicyView>, Error> {
    admin(&me)?;
    if let Some(why) = policy::invalid(&settings) {
        return Err(Error::invalid(why));
    }
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            policy::set(&tx, &settings, now)?;
            admin_event(&tx, now, "policy_changed", None, &me, &ip_text, "instance")?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    view(&state).await
}
