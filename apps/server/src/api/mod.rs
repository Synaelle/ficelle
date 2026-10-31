// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `/api/v1` endpoints for accounts, groups and admins. Versions,
//! headers and limits are handled around them in [`crate::routes`].

mod admin;
mod auth;
mod backups;
mod e2ee;
mod extensions;
pub mod extract;
mod feedback;
pub mod files;
mod groups;
mod invites;
mod me;
mod passkeys;
mod policies;
mod profile;
mod second_factor;
mod security;
mod setup;
mod sharing;
pub mod sso;
mod sync;
mod updates;

use app_protocol::account::User;
use axum::Router;
use axum::routing::{delete, get, patch, post, put};

use crate::accounts::store::UserRow;
use crate::routes::AppState;

/// Account routes, relative to `/api/v1`.
#[expect(clippy::too_many_lines, reason = "the route table, one line per route")]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/options", get(auth::options))
        .route("/setup", get(setup::status))
        .route("/setup/start", post(setup::start))
        .route("/setup/totp", post(setup::totp_setup))
        .route("/setup/passkey", post(setup::passkey_setup))
        .route("/setup/finish", post(setup::finish))
        .route("/setup/checks", post(setup::checks))
        .route("/setup/upload", post(setup::upload))
        .route("/setup/socket", get(setup::socket))
        .route("/admin/mail/test", post(admin::mail_test))
        .route("/admin/updates", get(updates::get).put(updates::set))
        .route("/admin/updates/check", post(updates::check))
        .route("/admin/checklist", get(updates::checklist))
        .route("/admin/checklist/dismiss", post(updates::dismiss))
        .route("/admin/checklist/restore", post(updates::restore))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/second-factor", post(second_factor::second_factor))
        .route("/auth/second-factor/email", post(second_factor::email_code))
        .route("/auth/kind", post(e2ee::kind))
        .route("/auth/opaque/registration", post(e2ee::registration))
        .route("/auth/e2ee/register", post(e2ee::register))
        .route("/auth/opaque/login/start", post(e2ee::login_start))
        .route("/auth/opaque/login/finish", post(e2ee::login_finish))
        .route("/auth/recovery/start", post(e2ee::recovery_start))
        .route("/auth/recovery/finish", post(e2ee::recovery_finish))
        .route("/me/confirm/start", post(e2ee::confirm_start))
        .route("/me/confirm/finish", post(e2ee::confirm_finish))
        .route("/me/keys", get(e2ee::keys))
        .route("/me/e2ee/password", post(e2ee::change_password))
        .route("/me/e2ee/strengthen", post(e2ee::strengthen))
        .route(
            "/me/email",
            put(profile::set_email).delete(profile::remove_email),
        )
        .route("/auth/email/verify", post(profile::verify_email))
        .route("/auth/reset/request", post(profile::reset_request))
        .route("/auth/reset/finish", post(profile::reset_finish))
        .route("/me/export", get(profile::export))
        .route(
            "/me/deletion",
            post(profile::request_deletion).delete(profile::cancel_deletion),
        )
        .route("/privacy", get(profile::privacy))
        .route("/auth/passkey/start", post(passkeys::sign_in_start))
        .route("/auth/passkey/finish", post(passkeys::sign_in_finish))
        .route("/auth/sso/start", post(sso::sign_in_start))
        .route("/auth/sso/exchange", post(sso::exchange))
        .route("/me/sso/link", post(sso::link_start))
        .route("/me/sso/{id}", delete(sso::unlink))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(me::me))
        .route("/me/password", post(me::change_password))
        .route("/me/devices", get(me::devices))
        .route("/me/security", get(security::security))
        .route("/me/totp/setup", post(security::totp_setup))
        .route("/me/totp/confirm", post(security::totp_confirm))
        .route("/me/totp/disable", post(security::totp_disable))
        .route("/me/email-codes/enable", post(security::email_codes_enable))
        .route(
            "/me/email-codes/disable",
            post(security::email_codes_disable),
        )
        .route("/me/recovery-codes", post(security::new_recovery_codes))
        .route("/me/passkeys/start", post(passkeys::add_start))
        .route("/me/passkeys/finish", post(passkeys::add_finish))
        .route("/me/passkeys/{id}", delete(passkeys::remove))
        .route("/me/devices/{id}", delete(me::revoke_device))
        .route("/invites", get(invites::list).post(invites::create))
        .route("/invites/{id}", delete(invites::revoke))
        .route("/me/groups", get(groups::mine))
        .route("/admin/overview", get(admin::overview))
        .route("/admin/users", get(admin::users).post(admin::create_user))
        .route(
            "/admin/users/{id}",
            get(admin::user)
                .patch(admin::update_user)
                .delete(admin::delete_user),
        )
        .route("/admin/users/{id}/password", post(admin::set_password))
        .route("/admin/users/{id}/signout", post(admin::sign_out))
        .route(
            "/admin/users/{id}/second-factors/reset",
            post(admin::reset_second_factors),
        )
        .route(
            "/admin/registration",
            get(admin::registration).put(admin::set_registration),
        )
        .route("/admin/audit", get(admin::audit))
        .route("/admin/account-kinds", put(admin::set_account_kinds))
        .route("/admin/coediting", put(admin::set_coediting))
        .route("/admin/second-step", put(admin::set_second_step))
        .route("/admin/personal", put(admin::set_personal))
        .merge(group_routes())
        .merge(
            sync_routes()
                .merge(extension_routes())
                .merge(feedback_routes()),
        )
}

/// Groups and the storage policies, relative to `/api/v1`.
fn group_routes() -> Router<AppState> {
    Router::new()
        .route("/admin/groups", get(groups::list).post(groups::create))
        .route(
            "/admin/groups/{id}",
            get(groups::detail)
                .patch(groups::rename)
                .delete(groups::delete),
        )
        .route("/admin/storage", get(policies::storage))
        .route(
            "/admin/policy",
            get(policies::instance).put(policies::set_instance),
        )
        .route(
            "/admin/groups/{id}/policy",
            get(policies::group).put(policies::set_group),
        )
        .route(
            "/admin/users/{id}/policy",
            get(policies::user).put(policies::set_user),
        )
        .route(
            "/admin/groups/{id}/members/{user}",
            put(groups::add_member).delete(groups::remove_member),
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
        .route("/admin/feedback/access", get(feedback::access))
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
            "/admin/feedback/readers/{name}",
            put(feedback::add_reader).delete(feedback::remove_reader),
        )
        .route(
            "/admin/feedback/token",
            post(feedback::new_token).delete(feedback::revoke_token),
        )
}

/// Themes the instance offers, backups and the catalog, relative to `/api/v1`.
fn extension_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/backups",
            get(backups::get)
                .put(backups::connect)
                .delete(backups::stop),
        )
        .route("/admin/backups/settings", put(backups::settings))
        .route("/admin/backups/run", post(backups::run))
        .route("/admin/backups/snapshots", get(backups::snapshots))
        .route("/admin/backups/check", post(backups::check))
        .route("/admin/backups/test-restore", post(backups::test_restore))
        .route(
            "/admin/backups/restore-person",
            post(backups::restore_person),
        )
        .route("/admin/extensions", get(extensions::list))
        .route("/admin/extensions/rules", put(extensions::set_rules))
        .route("/admin/extensions/{kind}", post(extensions::upload))
        .route(
            "/admin/extensions/{kind}/{id}",
            patch(extensions::set_preinstall).delete(extensions::remove),
        )
        .route(
            "/admin/catalog",
            get(extensions::admin_catalog).put(extensions::set_catalog),
        )
        .route("/sync/catalog", get(extensions::catalog_for_apps))
        .route("/sync/catalog/{kind}/{id}", get(extensions::catalog_file))
        .route("/sync/extensions", get(extensions::for_apps))
        .route("/sync/extensions/{kind}/{id}", get(extensions::download))
}

/// Sync and sharing routes, relative to `/api/v1`.
fn sync_routes() -> Router<AppState> {
    Router::new()
        .route("/sync", get(sync::info))
        .route("/sync/push", post(sync::push))
        .route("/sync/changes", get(sync::pull))
        .route("/sync/live", get(sync::live))
        .route("/sync/heavy", get(sync::heavy))
        .route("/sync/spaces", get(sharing::list).post(sharing::create))
        .route("/sync/spaces/{id}", delete(sharing::stop))
        .route("/sync/spaces/{id}/me", delete(sharing::leave))
        .route(
            "/sync/spaces/{id}/members",
            get(sharing::members).put(sharing::set_members),
        )
        .route("/sync/spaces/{id}/grants", post(sharing::give))
        .route("/sync/people", get(sharing::person))
        .route(
            "/sync/key-pair",
            get(sharing::key_pair).put(sharing::set_key_pair),
        )
        .route("/sync/files/policy", get(files::info))
        .route(
            "/sync/files/{address}",
            get(files::get).put(files::put).delete(files::release),
        )
        .route("/sync/files/{address}/state", get(files::state_of))
        .route("/sync/files/{address}/keep", post(files::keep))
}

pub use profile::run_deletions;

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
mod sharing_tests;
#[cfg(test)]
mod sso_tests;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transfer_tests;
#[cfg(test)]
mod updates_tests;
