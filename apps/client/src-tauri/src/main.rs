// SPDX-License-Identifier: AGPL-3.0-or-later

//! Desktop entry point.

// hides the extra console window on windows release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    client_lib::run();
}
