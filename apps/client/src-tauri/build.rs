// SPDX-License-Identifier: AGPL-3.0-or-later

//! Lets tauri embed its config, icons and capabilities into the binary,
//! turns on dev reports outside official releases, and leaves the updater out
//! of the APK an app store updates (`APP_STORE=1`).

fn main() {
    println!("cargo:rustc-check-cfg=cfg(dev_reports)");
    println!("cargo:rustc-check-cfg=cfg(app_store)");
    println!("cargo:rerun-if-env-changed=APP_RELEASE");
    println!("cargo:rerun-if-env-changed=APP_STORE");
    println!("cargo:rerun-if-env-changed=APP_REPORTS_DIR");
    if std::env::var("APP_RELEASE").as_deref() != Ok("1") {
        println!("cargo:rustc-cfg=dev_reports");
    }
    if std::env::var("APP_STORE").as_deref() == Ok("1") {
        println!("cargo:rustc-cfg=app_store");
    }
    tauri_build::build();
}
