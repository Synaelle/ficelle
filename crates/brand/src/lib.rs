// SPDX-License-Identifier: AGPL-3.0-or-later
// Written by tools/brand.mjs from packages/brand/brand.json. Edit that file, then run pnpm brand.

//! Product names and identifiers shared by every rust crate.
//!
//! Release and preview builds (`APP_RELEASE=1`, `APP_PREVIEW=1`) use the
//! real identity. Every other build uses the dev identity so it installs next
//! to the real app with its own data.

#[cfg(app_release)]
mod identity {
    //! Release identity.

    /// Name shown to users.
    pub const DISPLAY_NAME: &str = "Ficelle";
    /// Lowercase name used for folders, files and binaries.
    pub const SLUG: &str = "ficelle";
    /// Reverse domain app identifier (bundle id, package name).
    pub const IDENTIFIER: &str = "com.synaelle.ficelle";
}

#[cfg(not(app_release))]
mod identity {
    //! Dev identity.

    /// Name shown to users.
    pub const DISPLAY_NAME: &str = "Ficelle Dev";
    /// Lowercase name used for folders, files and binaries.
    pub const SLUG: &str = "ficelle-dev";
    /// Reverse domain app identifier (bundle id, package name).
    pub const IDENTIFIER: &str = "com.synaelle.ficelle.dev";
}

pub use identity::{DISPLAY_NAME, IDENTIFIER, SLUG};

/// Prefix of every environment variable, without the trailing underscore.
pub const ENV_PREFIX: &str = "FICELLE";
/// Server executable name.
pub const SERVER_BINARY: &str = "ficelle-server";
/// Server configuration file name.
pub const CONFIG_FILE: &str = "ficelle.toml";
/// Extension of backup archives, the same for every build so any of them
/// restores the others' backups.
pub const BACKUP_EXTENSION: &str = "ficelle";
/// Default instance that receives feedback reports.
pub const FEEDBACK_URL: &str = "https://ficelle.synaelle.com";
/// The official catalog of themes: the folder holding
/// `catalog.json`, ending with a slash.
pub const CATALOG_URL: &str = "https://ficelle.synaelle.com/catalog/";
/// Where the apps download fonts first: the project's own host, which
/// mirrors them (tools/fonts-mirror.mjs). jsDelivr only when it fails.
pub const FONTS_URL: &str = "https://ficelle.synaelle.com/fonts";
/// The hex Ed25519 public key the catalog's entries are signed with. Empty:
/// no catalog in this build.
pub const CATALOG_KEY: &str = "";
/// The signed list of releases servers check for update notices.
pub const RELEASES_URL: &str = "https://ficelle.synaelle.com/releases.json";
/// The hex Ed25519 public key its entries are signed with (the catalog's
/// may serve, the messages differ). Empty: no update check in this build.
pub const RELEASES_KEY: &str = "b87ba069a2e5ab6807cd59bd3f5c06186e62f3519a5d2bc16edbe0f458dcc0cc";
/// Where release apps look for a newer version of themselves: tauri's
/// `latest.json`, attached to the newest GitHub release.
pub const APP_UPDATES_URL: &str =
    "https://github.com/Synaelle/ficelle/releases/latest/download/latest.json";
/// The minisign public key (base64, as tauri writes it) the app files in it
/// are signed with. Empty: no app updates in this build.
pub const APP_UPDATES_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IENEOTEzOUVENDdFMzYyMjcKUldRbll1Tkg3VG1SelhSalBqMlF3KzA5WkU5SmJRZENHWjFkM2NvVE5hZGxQWWNXM2pRSGo4NlMK";

/// Version shown to users: `major.minor.micro` for releases,
/// `preview-{commit}` for a preview, `dev-{commit}` (maybe `-dirty`) otherwise.
pub const VERSION: &str = env!("APP_VERSION");
/// Date of the built commit, `YYYY-MM-DD`, empty outside git.
pub const BUILD_DATE: &str = env!("APP_BUILD_DATE");
