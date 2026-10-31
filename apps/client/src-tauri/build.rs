// SPDX-License-Identifier: AGPL-3.0-or-later

//! Lets tauri embed its config, icons and capabilities into the binary, and
//! turns on dev reports outside official releases.

fn main() {
    println!("cargo:rustc-check-cfg=cfg(dev_reports)");
    println!("cargo:rerun-if-env-changed=APP_RELEASE");
    println!("cargo:rerun-if-env-changed=APP_REPORTS_DIR");
    if std::env::var("APP_RELEASE").as_deref() != Ok("1") {
        println!("cargo:rustc-cfg=dev_reports");
    }
    tauri_build::build();
}
