// SPDX-License-Identifier: AGPL-3.0-or-later

//! Native shell around the web ui: exposes the core as commands and serves
//! attachment files.

#[cfg(target_os = "android")]
mod android_keystore;
mod biometric;
mod commands;
mod error;
#[cfg(windows)]
mod hello;
mod picked;
mod portable;
mod protocol;
mod state;
mod sync;

use tauri::Manager;

use crate::state::AppState;

/// Starts the app. Shared by the desktop binary and the mobile entry point.
///
/// # Panics
///
/// Panics when the webview cannot start, there is nothing useful to show then.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[expect(
    clippy::too_many_lines,
    reason = "the list of every command, read top to bottom"
)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "android")]
    let builder = builder
        .plugin(tauri_plugin_biometric::init())
        .plugin(android_keystore::pin_key_plugin());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    #[cfg(desktop)]
    let builder = builder.manage(commands::updates::Pending::default());
    builder
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_haptics::init())
        .setup(|app| {
            #[cfg(target_os = "android")]
            android_keystore::init(app.handle(), app.path().app_data_dir()?.join("keys"));
            let mut dir = portable::data_dir(app.handle())?.join("vault");
            // debug builds can open a test vault, so automated runs never
            // touch the vault of whoever develops on this machine
            if cfg!(debug_assertions)
                && let Some(test_dir) = std::env::var_os("APP_VAULT_DIR")
            {
                dir = test_dir.into();
            }
            // debug builds can run with a wrong clock, to test that sync
            // never needs a right one
            if cfg!(debug_assertions)
                && let Some(skew) = std::env::var("APP_CLOCK_SKEW_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
            {
                app_core::clock::set_skew(skew);
            }
            app.manage(AppState::new(dir));
            app.manage(sync::commands::PendingSignIn::default());
            app.manage(commands::import::Imports::default());
            // after manage: the thread reads the state from the handle
            app.state::<AppState>().sync.start(app.handle().clone());
            portable::open_main(app.handle())?;
            Ok(())
        })
        .register_uri_scheme_protocol(protocol::SCHEME, protocol::handle)
        .invoke_handler(tauri::generate_handler![
            sync::catalog::catalog_list,
            sync::feedback::feedback_where,
            sync::feedback::feedback_send,
            sync::feedback::feedback_answers,
            sync::catalog::catalog_theme,
            sync::extensions::instance_extensions,
            sync::extensions::instance_theme,
            commands::themes::themes_list,
            commands::themes::theme_save,
            commands::themes::theme_remove,
            commands::themes::read_text_file,
            commands::themes::write_text_file,
            commands::vault::vault_status,
            commands::vault::vault_create,
            commands::vault::vault_unlock,
            commands::vault::vault_lock,
            commands::vault::vault_reset,
            commands::vault::vault_unlock_biometric,
            commands::vault::vault_unlock_pin,
            commands::vault::vault_set_way,
            commands::vault::vault_encrypt,
            commands::fonts::font_download,
            commands::fonts::font_files,
            commands::fonts::font_file,
            commands::fonts::system_fonts,
            commands::accent::system_accent,
            commands::clock::system_clock_24h,
            commands::quick::quick_window_open,
            commands::updates::app_update_check,
            commands::updates::app_update_latest,
            commands::updates::app_update_install,
            commands::vault::vault_create_recovery,
            commands::vault::vault_remove_recovery,
            commands::vault::vault_recover,
            commands::vault::vault_change_password,
            commands::notes::list_notes,
            commands::notes::get_note,
            commands::notes::create_note,
            commands::notes::update_note,
            commands::notes::set_pinned,
            commands::notes::move_note,
            commands::notes::set_note_tags,
            commands::notes::trash_note,
            commands::notes::restore_note,
            commands::notes::purge_note,
            commands::notes::discard_if_blank,
            commands::files::attach_inbox_file,
            commands::notes::empty_trash,
            commands::notes::list_folders,
            commands::notes::create_folder,
            commands::notes::rename_folder,
            commands::notes::move_folder,
            commands::notes::trash_folder,
            commands::notes::restore_folder,
            commands::notes::list_tags,
            commands::notes::list_versions,
            commands::notes::restore_version,
            commands::notes::note_conflict,
            commands::notes::conflicted_notes,
            commands::notes::resolve_conflict,
            commands::links::search,
            commands::links::backlinks,
            commands::links::resolve_link,
            commands::links::link_suggestions,
            commands::links::note_headings,
            commands::files::add_attachment,
            commands::files::list_attachments,
            commands::files::all_attachments,
            commands::files::remove_attachment,
            commands::files::set_attachment_thumb,
            commands::files::export_markdown,
            commands::backups::backups_get,
            commands::backups::backups_connect,
            commands::backups::backups_settings,
            commands::backups::backups_disconnect,
            commands::backups::backups_run,
            commands::backups::backups_snapshots,
            commands::backups::backups_restore,
            commands::backups::backups_test_restore,
            commands::backups::backups_check,
            commands::backup::backup_default_name,
            commands::backup::backup_write,
            commands::backup::backup_encrypted,
            commands::backup::backup_inspect,
            commands::backup::backup_restore,
            commands::backup::export_markdown_zip,
            commands::import::import_open,
            commands::import::import_read,
            commands::import::import_known,
            commands::import::import_begin,
            commands::import::import_put,
            commands::import::import_write,
            commands::import::import_close,
            sync::files::attachment_files,
            sync::files::fetch_attachment,
            sync::files::save_attachment,
            sync::files::files_info,
            sync::files::files_refused,
            sync::files::files_set_auto_fetch,
            commands::dev_report::dev_report_create,
            commands::dev_report::dev_report_add_image,
            commands::dev_report::dev_report_folder,
            sync::commands::sync_check_server,
            sync::commands::sync_sign_in,
            sync::commands::sync_second_factor,
            sync::commands::sync_email_code,
            sync::commands::sync_status,
            sync::commands::sync_now,
            sync::commands::sync_viewing,
            sync::commands::sync_disconnect,
            sync::sharing::share_list,
            sync::sharing::share_find,
            sync::sharing::share_groups,
            sync::sharing::share_members,
            sync::sharing::share_create,
            sync::sharing::share_set_members,
            sync::sharing::share_stop,
            sync::sharing::share_waiting,
            sync::sharing::share_allow,
            sync::sharing::share_leave,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the app");
}
