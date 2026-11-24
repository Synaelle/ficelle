// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the apps' commands (`src-tauri/src/commands`) and the web version's
//! (`core-web`) share: the shapes the page reads and the steps between the
//! vault, the store and the files. Each shell passes in what its platform
//! has (key stores, a server's files policy) and keeps only how it reaches
//! its files and holds the open vault.

pub mod backup;
pub mod import;
pub mod vault;
