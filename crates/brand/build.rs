// SPDX-License-Identifier: AGPL-3.0-or-later

//! Stamps the version and build date users see, and picks the release or dev
//! identity. tools/version.mjs mirrors this logic for the ui, keep both in sync.
//!
//! - `APP_RELEASE=1`: the workspace version, release identity.
//! - `APP_PREVIEW=1`: `preview-{version}-{commit}`, release identity, for a
//!   build used day to day before the next release; the version is the
//!   release it is based on.
//! - `CI` set: `dev-{commit}`, dev identity, never dirty.
//! - otherwise: `dev-{commit}`, plus `-dirty` when the tree has local changes.
//!
//! `APP_COMMIT` and `APP_COMMIT_DATE` stand in for git where the sources
//! come without their history (the Docker image's build).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Same length as tools/version.mjs. Fixed so versions never change width.
const HASH_LEN: &str = "9";

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    watch(&root);

    println!("cargo:rustc-check-cfg=cfg(app_release)");
    let release = std::env::var("APP_RELEASE").as_deref() == Ok("1");
    let preview = std::env::var("APP_PREVIEW").as_deref() == Ok("1");
    let commit = given("APP_COMMIT")
        .map(|c| c.chars().take(HASH_LEN.parse().unwrap_or(9)).collect())
        .or_else(|| {
            git(
                &root,
                &["rev-parse", &format!("--short={HASH_LEN}"), "HEAD"],
            )
        })
        .unwrap_or_else(|| "unknown".into());

    if release || preview {
        println!("cargo:rustc-cfg=app_release");
    }
    let version = if release {
        env!("CARGO_PKG_VERSION").to_owned()
    } else if preview {
        format!("preview-{}-{commit}", env!("CARGO_PKG_VERSION"))
    } else if std::env::var_os("CI").is_some() {
        format!("dev-{commit}")
    } else {
        let dirty = git(&root, &["status", "--porcelain"]).is_some();
        format!("dev-{commit}{}", if dirty { "-dirty" } else { "" })
    };
    // the commit date rather than the wall clock keeps builds reproducible
    let date = given("APP_COMMIT_DATE")
        .or_else(|| git(&root, &["show", "-s", "--format=%cs", "HEAD"]))
        .unwrap_or_default();

    println!("cargo:rustc-env=APP_VERSION={version}");
    println!("cargo:rustc-env=APP_BUILD_DATE={date}");
}

/// Reruns when the commit, the index or any source changes, so the dirty flag
/// stays accurate without watching build output folders.
fn watch(root: &Path) {
    println!("cargo:rerun-if-env-changed=APP_RELEASE");
    println!("cargo:rerun-if-env-changed=APP_PREVIEW");
    println!("cargo:rerun-if-env-changed=CI");
    println!("cargo:rerun-if-env-changed=APP_COMMIT");
    println!("cargo:rerun-if-env-changed=APP_COMMIT_DATE");
    let watched: [PathBuf; 9] = [
        root.join(".git/HEAD"),
        root.join(".git/index"),
        root.join(".git/refs/heads"),
        root.join(".git/packed-refs"),
        root.join("crates"),
        root.join("apps/server/src"),
        root.join("apps/client/src"),
        root.join("apps/client/src-tauri/src"),
        root.join("Cargo.toml"),
    ];
    for path in watched.iter().filter(|path| path.exists()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// Runs git and returns trimmed stdout, or `None` on failure or empty output.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (output.status.success() && !text.is_empty()).then_some(text)
}

/// A non-empty environment variable.
fn given(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}
