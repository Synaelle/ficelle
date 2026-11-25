// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `/api/v1` endpoints for the account, sync and the admin. Versions,
//! headers and limits are handled around them in [`crate::routes`].

mod admin;
mod auth;
mod backups;
mod connect;
mod e2ee;
pub mod extract;
mod feedback;
pub mod files;
mod me;
mod passkeys;
mod policies;
mod profile;
mod second_factor;
mod security;
mod setup;
mod sync;
mod updates;

use app_protocol::account::User;
use axum::Router;
use axum::routing::{delete, get, post, put};

use crate::accounts::store::UserRow;
use crate::routes::AppState;

/// Account routes, relative to `/api/v1`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/options", get(auth::options))
        .route("/setup", get(setup::status))
        .route("/setup/start", post(setup::start))
        .route("/setup/totp", post(setup::totp_setup))
        .route("/setup/passkey", post(setup::passkey_setup))
        .route("/setup/registration", post(setup::registration))
        .route("/setup/finish", post(setup::finish))
        .route("/setup/checks", post(setup::checks))
        .route("/setup/upload", post(setup::upload))
        .route("/setup/socket", get(setup::socket))
        .route("/setup/restore/look", post(setup::restore_look))
        .route("/setup/restore", post(setup::restore))
        .route("/admin/updates", get(updates::get).put(updates::set))
        .route("/admin/updates/check", post(updates::check))
        .route("/admin/checklist", get(updates::checklist))
        .route("/admin/checklist/dismiss", post(updates::dismiss))
        .route("/admin/checklist/restore", post(updates::restore))
        .route("/auth/login", post(auth::login))
        .route("/auth/second-factor", post(second_factor::second_factor))
        .route("/auth/kind", post(e2ee::kind))
        .route("/auth/opaque/registration", post(e2ee::registration))
        .route("/auth/opaque/login/start", post(e2ee::login_start))
        .route("/auth/opaque/login/finish", post(e2ee::login_finish))
        .route("/auth/recovery/start", post(e2ee::recovery_start))
        .route("/auth/recovery/finish", post(e2ee::recovery_finish))
        .route("/me/confirm/start", post(e2ee::confirm_start))
        .route("/me/confirm/finish", post(e2ee::confirm_finish))
        .route("/me/keys", get(e2ee::keys))
        .route("/me/e2ee/strengthen", post(e2ee::strengthen))
        .route("/me/export", get(profile::export))
        .route("/privacy", get(profile::privacy))
        .route("/auth/passkey/start", post(passkeys::sign_in_start))
        .route("/auth/passkey/finish", post(passkeys::sign_in_finish))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(me::me))
        .route("/me/password", post(me::change_password))
        .route("/me/security", get(security::security))
        .route("/me/totp/setup", post(security::totp_setup))
        .route("/me/totp/confirm", post(security::totp_confirm))
        .route("/me/totp/disable", post(security::totp_disable))
        .route("/me/recovery-codes", post(security::new_recovery_codes))
        .route("/me/passkeys/start", post(passkeys::add_start))
        .route("/me/passkeys/finish", post(passkeys::add_finish))
        .route("/me/passkeys/{id}", delete(passkeys::remove))
        .route("/admin/overview", get(admin::overview))
        .route("/admin/audit", get(admin::audit))
        .route("/admin/coediting", put(admin::set_coediting))
        .route(
            "/admin/signin-limits",
            get(admin::signin_limits).put(admin::set_signin_limits),
        )
        .route("/auth/app/code", post(connect::code))
        .route("/auth/app/token", post(connect::token))
        .route("/auth/app/link", post(connect::link))
        .route("/auth/app/link/find", post(connect::link_find))
        .route("/auth/app/link/approve", post(connect::link_approve))
        .route("/auth/app/link/token", post(connect::link_token))
        .route("/auth/app/link/offer", post(connect::link_offer))
        .route(
            "/auth/app/link/offer/status",
            post(connect::link_offer_status),
        )
        .route(
            "/auth/app/link/offer/approve",
            post(connect::link_offer_approve),
        )
        .route("/admin/second-step", put(admin::set_second_step))
        .merge(policy_routes())
        .merge(
            sync_routes()
                .merge(backup_routes())
                .merge(feedback_routes()),
        )
}

/// The files policy and storage, relative to `/api/v1`.
fn policy_routes() -> Router<AppState> {
    Router::new()
        .route("/admin/storage", get(policies::storage))
        .route(
            "/admin/policy",
            get(policies::instance).put(policies::set_instance),
        )
}

/// Reports from anyone's apps, and the admin's switches, relative to `/api/v1`.
fn feedback_routes() -> Router<AppState> {
    Router::new()
        .route("/feedback/info", get(feedback::info))
        .route("/feedback/challenge", post(feedback::challenge))
        .route("/feedback/answers", post(feedback::answers))
        // the handler reads the body itself, with its own limit and time
        .route(
            "/feedback",
            post(feedback::send).layer(axum::extract::DefaultBodyLimit::disable()),
        )
        .route(
            "/admin/feedback/settings",
            get(feedback::settings).put(feedback::set_settings),
        )
        .route("/admin/feedback/caps", put(feedback::set_caps))
        // images, as a report from an app carries
        .route(
            "/admin/feedback/send",
            post(feedback::send_to_main)
                .layer(axum::extract::DefaultBodyLimit::max(feedback::MAX_REQUEST)),
        )
        .route("/admin/feedback/reports", get(feedback::reports))
        .route(
            "/admin/feedback/reports/{id}",
            get(feedback::report)
                .patch(feedback::change)
                .delete(feedback::remove),
        )
        .route(
            "/admin/feedback/reports/{id}/images/{position}",
            get(feedback::image),
        )
        .route(
            "/admin/feedback/reports/{id}/block",
            post(feedback::block_sender),
        )
        .route(
            "/admin/feedback/blocks",
            get(feedback::blocks)
                .post(feedback::add_block)
                .delete(feedback::remove_block),
        )
        .route(
            "/admin/feedback/token",
            post(feedback::new_token).delete(feedback::revoke_token),
        )
}

/// Backups, relative to `/api/v1`.
fn backup_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/backups",
            get(backups::get)
                .put(backups::connect)
                .delete(backups::stop),
        )
        .route("/admin/backups/settings", put(backups::settings))
        .route("/admin/backups/key", put(backups::lock))
        .route("/admin/backups/run", post(backups::run))
        .route("/admin/backups/snapshots", get(backups::snapshots))
        .route("/admin/backups/check", post(backups::check))
        .route("/admin/backups/test-restore", post(backups::test_restore))
}

/// Sync routes, relative to `/api/v1`.
fn sync_routes() -> Router<AppState> {
    Router::new()
        .route("/sync", get(sync::info))
        .route("/sync/push", post(sync::push))
        .route("/sync/changes", get(sync::pull))
        .route("/sync/live", get(sync::live))
        .route("/sync/heavy", get(sync::heavy))
        .route("/sync/files/policy", get(files::info))
        .route(
            "/sync/files/{address}",
            get(files::get).put(files::put).delete(files::release),
        )
        .route("/sync/files/{address}/state", get(files::state_of))
        .route("/sync/files/{address}/keep", post(files::keep))
}

/// An account as its owner and admins see it.
fn user_json(user: &UserRow) -> User {
    User {
        id: user.id.clone(),
        username: user.username.clone(),
        display_name: user.display_name.clone(),
        role: user.role,
        kind: user.kind,
        created_at: user.created_at,
        factor_required: user.factor_required,
    }
}

#[cfg(test)]
mod admin_tests;
#[cfg(test)]
mod backups_tests;
#[cfg(test)]
mod connect_tests;
#[cfg(test)]
mod e2ee_tests;
#[cfg(test)]
mod feedback_tests;
#[cfg(test)]
mod files_tests;
#[cfg(test)]
mod passkey_tests;
#[cfg(test)]
mod policies_tests;
#[cfg(test)]
mod profile_tests;
#[cfg(test)]
mod security_tests;
#[cfg(test)]
mod setup_attack_tests;
#[cfg(test)]
mod setup_tests;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transfer_tests;
#[cfg(test)]
mod updates_tests;
