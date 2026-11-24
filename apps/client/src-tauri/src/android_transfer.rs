// SPDX-License-Identifier: AGPL-3.0-or-later

//! A download the person asked for, in Android's notifications while it
//! lasts (`TransferPlugin` in Transfer.kt). The service behind it keeps the
//! app running with the screen off or another app in front, so a large
//! file still arrives.

use std::sync::OnceLock;

use serde::Serialize;
use tauri::Wry;
use tauri::plugin::{PluginHandle, TauriPlugin};

static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();

/// The in-app plugin. No permission names it, so the page cannot call it.
pub fn plugin() -> TauriPlugin<Wry> {
    tauri::plugin::Builder::new("file-transfer")
        .setup(|_app, api| {
            let handle =
                api.register_android_plugin("com.synaelle.ficelle.dev", "TransferPlugin")?;
            let _ = HANDLE.set(handle);
            Ok(())
        })
        .build()
}

#[derive(Serialize)]
struct Shown<'a> {
    name: &'a str,
    done: u64,
    total: u64,
}

/// Shows the file and how far it got, starting the service the first time.
pub fn show(name: &str, done: u64, total: u64) {
    if let Some(handle) = HANDLE.get()
        && let Err(err) =
            handle.run_mobile_plugin::<serde_json::Value>("show", Shown { name, done, total })
    {
        // the download goes on without it while the app stays in front
        eprintln!("download notification: {err}");
    }
}

/// Ends the notification and the service.
pub fn stop() {
    if let Some(handle) = HANDLE.get()
        && let Err(err) = handle.run_mobile_plugin::<serde_json::Value>("stop", ())
    {
        eprintln!("download notification: {err}");
    }
}
