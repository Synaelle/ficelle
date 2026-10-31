// SPDX-License-Identifier: AGPL-3.0-or-later

//! Dev helper for testing alone. Each profile is a separate local vault under
//! `.dev-data/<profile>`, standing in for one device. Run with `--help`.

mod catalog;
mod releases;
mod seed;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use app_core::crypto::KdfParams;
use app_core::export::{NoAttachments, export_markdown};
use app_core::model::{MATCH_END, MATCH_START, NoteScope, NoteSort};
use app_core::store::Store;
use app_core::vault::{Encryption, Vault};

const HELP: &str = "devtool, local dev helper

usage: devtool [--profile NAME] [--data DIR] COMMAND

commands:
  init [--encrypt]     create the profile, encrypted with DEVTOOL_PASSWORD (default dev-password)
  seed [--count N]     add the sample notes, plus N generated ones (default 0)
  list [--trash]       list notes, pinned first
  search WORDS...      full text search
  export DIR           write the profile as markdown into an empty DIR
  stats                note, folder and tag counts, sync documents and pending changes
  relay FROM           apply profile FROM's pending sync changes to this profile, as a
                       server would (FROM keeps them, relaying twice changes nothing)
  reset                delete the profile data
  profiles             list profiles
  catalog keygen KEYFILE
                       make the official catalog's key: the private key goes to KEYFILE
                       (keep it off the repository, back it up), the public one is printed
                       for brand.json, catalogKey
  catalog sign FOLDER KEYFILE
                       sign FOLDER/themes/*.json into FOLDER/catalog.json,
                       then upload FOLDER to catalogUrl
  releases sign FILE KEYFILE
                       check and sign every release in FILE (releases.json: version,
                       date, security, notes, url), newest first, then upload it to
                       releasesUrl. The catalog's key may serve, set releasesKey too

profiles default to 'laptop'. Use one profile per simulated device.";

struct Args {
    profile: String,
    data: PathBuf,
    command: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut profile = String::from("laptop");
    // the repo root is two levels above this crate
    let mut data = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map_or_else(|| PathBuf::from(".dev-data"), |root| root.join(".dev-data"));
    let mut command = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--profile" => profile = args.next().ok_or("--profile needs a name")?,
            "--data" => data = args.next().ok_or("--data needs a folder")?.into(),
            "-h" | "--help" => command = vec!["help".into()],
            _ => command.push(arg),
        }
    }
    if profile.is_empty()
        || !profile
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("profile names use letters, digits and dashes".into());
    }
    Ok(Args {
        profile,
        data,
        command,
    })
}

fn password() -> String {
    std::env::var("DEVTOOL_PASSWORD").unwrap_or_else(|_| "dev-password".into())
}

fn unlock(dir: &Path) -> app_core::Result<Store> {
    let vault = Vault::open(dir)?;
    if vault.is_encrypted() {
        vault.unlock_with_password(&password())
    } else {
        vault.unlock_plain()
    }
}

fn init(dir: &Path, encrypt: bool) -> app_core::Result<()> {
    let password = password();
    let encryption = if encrypt {
        Encryption::On {
            password: &password,
            kdf: KdfParams::DEFAULT,
        }
    } else {
        Encryption::Off
    };
    let (vault, _) = Vault::create(dir, encryption)?;
    let kind = if vault.is_encrypted() {
        "encrypted"
    } else {
        "plain"
    };
    println!("created {} ({kind})", dir.display());
    Ok(())
}

fn list(dir: &Path, trash: bool) -> app_core::Result<()> {
    let scope = if trash {
        NoteScope::Trash
    } else {
        NoteScope::All
    };
    for note in unlock(dir)?.list_notes(scope, NoteSort::UpdatedDesc)? {
        let pin = if note.pinned { "*" } else { " " };
        let title = if note.title.is_empty() {
            "(untitled)"
        } else {
            &note.title
        };
        println!("{pin} {}  {title}", note.id);
    }
    Ok(())
}

fn search(dir: &Path, query: &str) -> app_core::Result<()> {
    for hit in unlock(dir)?.search(query, 20)? {
        let snippet = hit
            .snippet
            .replace(MATCH_START, "[")
            .replace(MATCH_END, "]");
        println!(
            "{}  {}
    {snippet}",
            hit.note_id, hit.title
        );
    }
    Ok(())
}

fn profiles(data: &Path) {
    let entries = std::fs::read_dir(data)
        .map(Iterator::collect::<Vec<_>>)
        .unwrap_or_default();
    for entry in entries
        .into_iter()
        .flatten()
        .filter(|e| Vault::exists(&e.path()))
    {
        println!("{}", entry.file_name().to_string_lossy());
    }
}

fn run(args: &Args) -> Result<(), String> {
    let dir = args.data.join(&args.profile);
    let words: Vec<&str> = args.command.iter().map(String::as_str).collect();
    let fail = |err: app_core::Error| err.to_string();
    match words.as_slice() {
        [] | ["help"] => println!("{HELP}"),
        ["catalog", "keygen", keyfile] => {
            let public = catalog::keygen(Path::new(keyfile))?;
            println!("private key written to {keyfile}, keep it safe and off the repository");
            println!(
                "public key for brand.json, catalogKey:
{public}"
            );
        }
        ["catalog", "sign", folder, keyfile] => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
            let themes = catalog::sign_folder(Path::new(folder), Path::new(keyfile), now)?;
            println!("signed {themes} themes into {folder}/catalog.json");
        }
        ["releases", "sign", file, keyfile] => {
            let count = releases::sign_file(Path::new(file), Path::new(keyfile))?;
            println!("signed {count} releases into {file}");
        }
        ["init", rest @ ..] => init(&dir, rest.contains(&"--encrypt")).map_err(fail)?,
        ["seed", rest @ ..] => {
            let count = match rest {
                ["--count", n] => n.parse().map_err(|_| "--count needs a number")?,
                [] => 0,
                _ => return Err("usage: seed [--count N]".into()),
            };
            let added = seed::seed(&mut unlock(&dir).map_err(fail)?, count).map_err(fail)?;
            println!("added {added} notes to {}", args.profile);
        }
        ["list", rest @ ..] => list(&dir, rest.contains(&"--trash")).map_err(fail)?,
        ["search", query @ ..] if !query.is_empty() => {
            search(&dir, &query.join(" ")).map_err(fail)?;
        }
        ["export", out] => {
            let store = unlock(&dir).map_err(fail)?;
            let report = export_markdown(&store, Path::new(out), &NoAttachments).map_err(fail)?;
            println!(
                "exported {} notes in {} folders to {out}",
                report.notes, report.folders
            );
        }
        ["relay", from] => {
            let mut source = unlock(&args.data.join(from)).map_err(fail)?;
            let mut target = unlock(&dir).map_err(fail)?;
            let changes = source.sync_outbox(usize::MAX).map_err(fail)?;
            for change in &changes {
                target
                    .apply_remote(&change.doc_id, &change.bytes)
                    .map_err(fail)?;
            }
            let (notes, folders, tags) = target.counts().map_err(fail)?;
            println!(
                "relayed {} changes from {from} to {}: now {notes} notes, {folders} folders, {tags} tags",
                changes.len(),
                args.profile
            );
        }
        ["stats"] => {
            let store = unlock(&dir).map_err(fail)?;
            let (notes, folders, tags) = store.counts().map_err(fail)?;
            let sync = store.sync_counts().map_err(fail)?;
            println!(
                "{}: {notes} notes, {folders} folders, {tags} tags; sync: {} documents, {} changes waiting ({} KB)",
                args.profile,
                sync.documents,
                sync.pending,
                sync.pending_bytes / 1024
            );
        }
        ["reset"] => {
            if dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
            }
            println!("removed {}", dir.display());
        }
        ["profiles"] => profiles(&args.data),
        _ => {
            return Err(format!(
                "unknown command, see --help

{HELP}"
            ));
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match parse_args().and_then(|args| run(&args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
