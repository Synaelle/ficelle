// SPDX-License-Identifier: AGPL-3.0-or-later

//! Themes for the instance: the admin's rule and uploads
//! (`/admin/extensions`), what the apps signed in read and download
//! (`/sync/extensions`). The apps keep the rule, see [`crate::extensions`].

use app_protocol::catalog::{CatalogKind, CatalogView, SetCatalog};
use app_protocol::extensions::{ExtensionRules, Extensions, SetPreinstall};
use axum::Json;
use axum::body::{Body as HttpBody, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use serde::Deserialize;

use super::admin::{admin, admin_event};
use super::extract::{Body, ClientIp, CurrentUser};
use crate::catalog;
use crate::error::Error;
use crate::extensions::{self as store, Refused, THEMES};
use crate::routes::AppState;

/// Only themes are offered: any other kind in a route is not found.
fn themes_only(route: &str) -> Result<(), Error> {
    if route == THEMES {
        Ok(())
    } else {
        Err(Error::not_found())
    }
}

fn gone() -> Error {
    Error::new(
        StatusCode::NOT_FOUND,
        app_protocol::error_code::NOT_FOUND,
        "no such theme",
    )
}

/// `GET /api/v1/admin/extensions`
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<Extensions>, Error> {
    admin(&me)?;
    Ok(Json(state.db.call(|c| store::all(c)).await?))
}

/// `PUT /api/v1/admin/extensions/rules`
pub async fn set_rules(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(rules): Body<ExtensionRules>,
) -> Result<Json<Extensions>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    Ok(Json(
        state
            .db
            .call(move |c| {
                let tx = c.transaction()?;
                store::set_rules(&tx, &rules)?;
                admin_event(&tx, now, "extension_rules", None, &me, &ip_text, "")?;
                tx.commit()?;
                store::all(c)
            })
            .await?,
    ))
}

/// Whether an upload is offered on every device.
#[derive(Debug, Deserialize)]
pub struct UploadQuery {
    #[serde(default)]
    preinstall: bool,
}

/// `POST /api/v1/admin/extensions/themes?preinstall=`: the theme file as
/// the body.
pub async fn upload(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path(route): Path<String>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Result<(StatusCode, Json<Extensions>), Error> {
    admin(&me)?;
    themes_only(&route)?;
    let (id, name) = store::read_theme(&body).map_err(|Refused(why)| Error::invalid(why))?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let all = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let upload = store::Upload {
                id: &id,
                name: &name,
                content: &body,
                preinstall: q.preinstall,
            };
            store::put(&tx, &upload, now)?;
            admin_event(
                &tx,
                now,
                "extension_added",
                None,
                &me,
                &ip_text,
                &format!("{route} {id}"),
            )?;
            tx.commit()?;
            store::all(c)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(all)))
}

/// `PATCH /api/v1/admin/extensions/themes/{id}`
pub async fn set_preinstall(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path((route, id)): Path<(String, String)>,
    Body(change): Body<SetPreinstall>,
) -> Result<Json<Extensions>, Error> {
    admin(&me)?;
    themes_only(&route)?;
    let found = state
        .db
        .call(move |c| store::set_preinstall(c, &id, change.preinstall))
        .await?;
    if !found {
        return Err(gone());
    }
    Ok(Json(state.db.call(|c| store::all(c)).await?))
}

/// `DELETE /api/v1/admin/extensions/themes/{id}`
pub async fn remove(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Path((route, id)): Path<(String, String)>,
) -> Result<Json<Extensions>, Error> {
    admin(&me)?;
    themes_only(&route)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    let found = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let found = store::remove(&tx, &id)?;
            if found {
                let what = format!("{route} {id}");
                admin_event(&tx, now, "extension_removed", None, &me, &ip_text, &what)?;
            }
            tx.commit()?;
            Ok(found)
        })
        .await?;
    if !found {
        return Err(gone());
    }
    Ok(Json(state.db.call(|c| store::all(c)).await?))
}

/// `GET /api/v1/sync/extensions`: what every app signed in follows.
pub async fn for_apps(
    State(state): State<AppState>,
    CurrentUser(_me): CurrentUser,
) -> Result<Json<Extensions>, Error> {
    Ok(Json(state.db.call(|c| store::all(c)).await?))
}

/// `GET /api/v1/sync/extensions/themes/{id}`: the file as uploaded.
pub async fn download(
    State(state): State<AppState>,
    CurrentUser(_me): CurrentUser,
    Path((route, id)): Path<(String, String)>,
) -> Result<Response, Error> {
    themes_only(&route)?;
    let content = state
        .db
        .call(move |c| store::content(c, &id))
        .await?
        .ok_or_else(gone)?;
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(HttpBody::from(content))
        .map_err(|_| Error::invalid("cannot send it"))
}

/// The catalog for the apps and the admin page.
async fn catalog_view(state: &AppState, force: bool) -> Result<CatalogView, Error> {
    let enabled = state.db.call(|c| catalog::enabled(c)).await?;
    Ok(state.catalog.view(enabled, state.clock.now(), force).await)
}

/// `GET /api/v1/admin/catalog`
pub async fn admin_catalog(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
) -> Result<Json<CatalogView>, Error> {
    admin(&me)?;
    Ok(Json(catalog_view(&state, false).await?))
}

/// `PUT /api/v1/admin/catalog`: turned on, it is fetched at once.
pub async fn set_catalog(
    State(state): State<AppState>,
    ip: ClientIp,
    CurrentUser(me): CurrentUser,
    Body(change): Body<SetCatalog>,
) -> Result<Json<CatalogView>, Error> {
    admin(&me)?;
    let now = state.clock.now();
    let ip_text = ip.0.to_string();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            catalog::set_enabled(&tx, change.enabled)?;
            let what = if change.enabled { "on" } else { "off" };
            admin_event(&tx, now, "catalog_changed", None, &me, &ip_text, what)?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(Json(catalog_view(&state, change.enabled).await?))
}

/// `GET /api/v1/sync/catalog`
pub async fn catalog_for_apps(
    State(state): State<AppState>,
    CurrentUser(_me): CurrentUser,
) -> Result<Json<CatalogView>, Error> {
    Ok(Json(catalog_view(&state, false).await?))
}

/// `GET /api/v1/sync/catalog/themes/{id}`: the file, checked.
pub async fn catalog_file(
    State(state): State<AppState>,
    CurrentUser(_me): CurrentUser,
    Path((route, id)): Path<(String, String)>,
) -> Result<Response, Error> {
    let (kind, media) = match route.as_str() {
        "themes" => (CatalogKind::Theme, "application/json"),
        _ => return Err(Error::not_found()),
    };
    if !state.db.call(|c| catalog::enabled(c)).await? {
        return Err(gone());
    }
    let bytes = state
        .catalog
        .file(kind, &id)
        .await
        .map_err(|why| {
            Error::new(
                StatusCode::BAD_GATEWAY,
                app_protocol::error_code::INVALID,
                why,
            )
        })?
        .ok_or_else(gone)?;
    Response::builder()
        .header(header::CONTENT_TYPE, media)
        .header(header::CACHE_CONTROL, "no-store")
        .body(HttpBody::from(bytes))
        .map_err(|_| Error::invalid("cannot send it"))
}
