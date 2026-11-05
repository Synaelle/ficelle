// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tells WebView2 how much memory it may keep. The app itself forbids unsafe
//! code, and calling WebView2 cannot be done without it, so the one call lives
//! here on its own.

#![cfg(windows)]

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    ICoreWebView2_19, ICoreWebView2Controller,
};
use windows_core::Interface;

/// Low: the WebView gives back what it can, for a window nobody is looking at.
/// An older WebView2 without this setting answers with an error, the caller
/// goes on without it.
pub fn set_low(controller: &ICoreWebView2Controller, low: bool) -> windows_core::Result<()> {
    let level = if low {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    };
    // SAFETY: the controller is alive for this borrow, these WebView2 calls
    // take nothing but a plain value, and tauri's with_webview, the only caller,
    // runs on the thread WebView2 requires
    unsafe {
        let webview = controller.CoreWebView2()?.cast::<ICoreWebView2_19>()?;
        webview.SetMemoryUsageTargetLevel(level)
    }
}
