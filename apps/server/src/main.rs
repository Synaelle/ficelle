// SPDX-License-Identifier: AGPL-3.0-or-later

//! Server entry point: reads the configuration, opens the database and
//! serves. `--check` stops after both succeed, for deploy scripts.

mod accounts;
mod admin;
mod api;
mod backups;
mod catalog;
mod cli;
mod clock;
mod config;
mod db;
mod error;
mod extensions;
mod feedback;
mod files;
mod keys;
mod live;
mod mail;
mod passkeys;
mod routes;
mod secrets;
mod setup;
mod sftp;
mod sharing;
mod sso;
mod sync;
mod updates;
mod web;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tracing_subscriber::EnvFilter;

use crate::config::{Config, LogFormat};
use crate::db::Db;
use crate::routes::AppState;

const USAGE: &str = "usage: ficelle-server [--config <file>] [--check] [--version]
       ficelle-server [--config <file>] setup [--force]
       ficelle-server [--config <file>] admin create <name> [--display-name <name>]
                      [--password-file <file>]
       ficelle-server [--config <file>] admin reset <name>
       ficelle-server [--config <file>] healthcheck
       ficelle-server backup list|check|restore --target <file> [...]

  --config <file>  configuration file, default ficelle.toml in the working
                   folder when it exists ({ENV}_CONFIG works too)
  --check          load the configuration and open the database, then exit
  --version        print the version and exit

  setup            asks the usual questions and writes the configuration file
                   (--config, or ficelle.toml here), readable by its owner only
  admin create <name>
                   an admin with a password alone, read from --password-file
                   (a Docker secret) or asked twice. At its first sign-in it
                   adds a passkey or an authenticator app before anything
                   else. The first admin locks setup.
  admin reset <name>
                   for an admin locked out: a new password printed once, its
                   passkeys and authenticator app removed (a new one is due
                   at the next sign-in), every device signed out
  healthcheck      exits 0 when the running server answers on its address,
                   for container health checks

  These work while the server runs, with docker exec too.

  backup list --target <file>
                   the instance's backups at the place <file> describes (the
                   json the admin page shows under Backups, its secrets
                   filled in). The password comes from {ENV}_BACKUP_PASSWORD,
                   else from the first line of the input.
  backup check --target <file> [--full]
                   checks the backups, reading every file with --full
  backup restore --target <file> --into <folder> [--snapshot <id>]
                   restores a backup (the newest without --snapshot) into an
                   empty folder: the database, the master key, every file.
                   Point storage.data_dir at it, the server stopped.

Every key can also come from {ENV}_SECTION__KEY.";

struct Args {
    config: Option<PathBuf>,
    check: bool,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut args = Args {
        config: std::env::var_os(format!("{}_CONFIG", app_brand::ENV_PREFIX)).map(PathBuf::from),
        check: false,
    };
    let mut rest = std::env::args().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--config" => args.config = Some(rest.next().ok_or("--config needs a file")?.into()),
            "--check" => args.check = true,
            "--version" => {
                println!("{} {}", app_brand::SERVER_BINARY, app_brand::VERSION);
                return Ok(None);
            }
            "--help" | "-h" => {
                println!(
                    "{}",
                    USAGE
                        .replace("{ENV}", app_brand::ENV_PREFIX)
                        .replace("ficelle-server", app_brand::SERVER_BINARY)
                );
                return Ok(None);
            }
            other => return Err(format!("unknown argument {other}, see --help")),
        }
    }
    Ok(Some(args))
}

/// `backup ...` from the command line, for a server that may be stopped
/// or gone: nothing but the place and the password is needed.
fn backup_command(words: &[String]) -> Result<(), String> {
    let mut target_file = None;
    let mut into = None;
    let mut snapshot = None;
    let mut full = false;
    let mut rest = words[1..].iter();
    while let Some(word) = rest.next() {
        match word.as_str() {
            "--target" => target_file = Some(rest.next().ok_or("--target needs a file")?),
            "--into" => into = Some(rest.next().ok_or("--into needs a folder")?),
            "--snapshot" => snapshot = Some(rest.next().ok_or("--snapshot needs an id")?.clone()),
            "--full" => full = true,
            other => return Err(format!("unknown argument {other}, see --help")),
        }
    }
    let text = std::fs::read_to_string(target_file.ok_or("--target is needed")?)
        .map_err(|e| e.to_string())?;
    let target: backups::ServerTarget =
        serde_json::from_str(&text).map_err(|e| format!("the target file: {e}"))?;
    let password =
        if let Ok(p) = std::env::var(format!("{}_BACKUP_PASSWORD", app_brand::ENV_PREFIX)) {
            p
        } else {
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| e.to_string())?;
            line.trim_end().to_owned()
        };
    let repo = app_repo::Repo::open(target.open()?, &password).map_err(|e| e.to_string())?;
    let mut list: Vec<app_repo::Snapshot> = repo
        .snapshots()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|s| s.kind == "server")
        .collect();
    match words[0].as_str() {
        "list" => {
            for s in &list {
                println!(
                    "{}  {}  {} people, {} files, {} bytes",
                    s.id,
                    s.time,
                    s.counts.get("users").copied().unwrap_or(0),
                    s.counts.get("files").copied().unwrap_or(0),
                    s.size()
                );
            }
            Ok(())
        }
        "check" => {
            let report = repo.check(full).map_err(|e| e.to_string())?;
            for problem in &report.problems {
                println!("{problem}");
            }
            if report.problems.is_empty() {
                println!(
                    "{} snapshots, {} packs, nothing wrong",
                    report.snapshots, report.packs
                );
                Ok(())
            } else {
                Err("the backups are damaged".into())
            }
        }
        "restore" => {
            let into = into.ok_or("--into is needed")?;
            let id = match snapshot {
                Some(id) => id,
                None => list
                    .pop()
                    .map(|s| s.id)
                    .ok_or("there is no backup of an instance there")?,
            };
            let restored = backups::instance::restore(&repo, &id, std::path::Path::new(into))?;
            println!(
                "restored {id} into {into}: {} people, {} files",
                restored.counts.get("users").copied().unwrap_or(0),
                restored.counts.get("files").copied().unwrap_or(0)
            );
            Ok(())
        }
        other => Err(format!("unknown backup command {other}, see --help")),
    }
}

/// `--config <file>` taken out of the words, the commands after it.
fn split_config(words: &[String]) -> Result<(Option<PathBuf>, Vec<String>), String> {
    let mut config =
        std::env::var_os(format!("{}_CONFIG", app_brand::ENV_PREFIX)).map(PathBuf::from);
    let mut rest = Vec::new();
    let mut iter = words.iter();
    while let Some(word) = iter.next() {
        if word == "--config" {
            config = Some(iter.next().ok_or("--config needs a file")?.into());
        } else {
            rest.push(word.clone());
        }
    }
    Ok((config, rest))
}

/// `setup`, `admin` and `healthcheck`. `None` when the words are none of them.
async fn command(words: &[String]) -> Option<Result<(), String>> {
    let (path, rest) = match split_config(words) {
        Ok(split) => split,
        Err(why) => return Some(Err(why)),
    };
    let name = rest.first()?.clone();
    if !matches!(name.as_str(), "setup" | "admin" | "healthcheck") {
        return None;
    }
    if name == "setup" {
        let target = path.unwrap_or_else(|| PathBuf::from(app_brand::CONFIG_FILE));
        return Some(cli::setup_command(&rest[1..], &target));
    }
    let config = match Config::load(path.as_deref()) {
        Ok(config) => config,
        Err(err) => return Some(Err(format!("configuration: {err}"))),
    };
    Some(if name == "healthcheck" {
        cli::healthcheck(&config)
    } else {
        cli::admin_command(&rest[1..], &config).await
    })
}

#[tokio::main]
async fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    match command(&words).await {
        Some(Ok(())) => return,
        Some(Err(why)) => fail(&why, 1),
        None => {}
    }
    if words.first().map(String::as_str) == Some("backup") && words.len() > 1 {
        let rest = words[1..].to_vec();
        // the targets block, SFTP runs its own runtime: off this one
        match tokio::task::spawn_blocking(move || backup_command(&rest)).await {
            Ok(Ok(())) => return,
            Ok(Err(why)) => fail(&why, 1),
            Err(why) => fail(&why.to_string(), 1),
        }
    }
    let args = match parse_args() {
        Ok(Some(args)) => args,
        Ok(None) => return,
        Err(err) => fail(&err, 2),
    };
    let config = load_config(args.config.as_deref());
    let filter = EnvFilter::try_new(&config.log.level).unwrap_or_else(|_| "info".into());
    match config.log.format {
        LogFormat::Text => tracing_subscriber::fmt().with_env_filter(filter).init(),
        LogFormat::Json => tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .init(),
    }

    let (db, feedback, master) = open_stores(&config, args.config.as_deref());
    if args.check {
        println!("configuration, master key and database ok");
        return;
    }
    if config.setup.allow_http {
        tracing::warn!(
            "setup.allow_http is on: the setup link and the admin password may cross the network unencrypted"
        );
    }

    let addr = config.server.bind;
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => fail(&format!("cannot listen on {addr}: {err}"), 1),
    };
    tracing::info!(
        "{} {} listening on {addr}",
        app_brand::SERVER_BINARY,
        app_brand::VERSION
    );

    let files_dir = config.storage.data_dir.join("files");
    let mut state = AppState::new(config, db);
    state.master = std::sync::Arc::new(master);
    state.feedback = feedback;
    state.files = std::sync::Arc::new(files::LocalDisk::new(files_dir));
    match state.db.call(|c| setup::is_locked(c)).await {
        Ok(true) => state.setup.set_done(),
        Ok(false) if !state.config.setup.admin.is_empty() => {
            if let Err(why) = automated_setup(&state).await {
                fail(&format!("automated setup: {why}"), 1);
            }
        }
        Ok(false) => spawn_setup_links(state.clone()),
        Err(err) => fail(&err.to_string(), 1),
    }
    spawn_cleanup(state.clone());
    backups::remove_leftover_scratch(&state.config.storage.data_dir);
    let master = state.master.clone();
    match state
        .db
        .call(move |c| accounts::factors::seal_plain_totp(c, &master))
        .await
    {
        Ok(0) => {}
        Ok(n) => tracing::info!("{n} authenticator app secrets sealed by the master key"),
        Err(e) => tracing::warn!("sealing the authenticator app secrets: {e}"),
    }
    backups::spawn_schedule(state.clone());
    updates::spawn(state.clone());
    let app = routes::router(state).into_make_service_with_connect_info::<SocketAddr>();
    if let Err(err) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        fail(&format!("server stopped: {err}"), 1);
    }
}

/// `[setup] admin`: the first start makes that admin from a password file
/// instead of printing a setup link.
async fn automated_setup(state: &AppState) -> Result<(), String> {
    let setup = &state.config.setup;
    let password = setup.admin_password()?;
    cli::create_admin(
        &state.db,
        &state.config,
        &setup.admin,
        None,
        &password,
        cli::Origin::Automated,
    )
    .await?;
    state.setup.set_done();
    tracing::warn!(
        "admin {} made from setup.admin, setup is locked. At its first sign-in on the admin page it adds a passkey or an authenticator app. The password file can go now.",
        setup.admin.trim().to_ascii_lowercase()
    );
    Ok(())
}

/// While setup is pending, prints a setup link now and a new one whenever
/// the last ran out unused. Only whoever reads the log gets it.
fn spawn_setup_links(state: AppState) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(60));
        loop {
            every.tick().await;
            if state.setup.is_done() {
                return;
            }
            let now = state.clock.now();
            if state.setup.link_needed(now) {
                let token = state.setup.new_link(now);
                tracing::warn!(
                    "no admin yet. Open this setup link within the hour, it works once: {}",
                    setup::link_text(&state.config, &token)
                );
                if state.config.server.public_url.is_empty() {
                    tracing::warn!(
                        "server.public_url is not set: use the address people will reach the server at, with /setup#token=... after it"
                    );
                }
            }
        }
    });
}

/// Expired tokens go once an hour.
fn spawn_cleanup(state: AppState) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(3600));
        loop {
            every.tick().await;
            let now = state.clock.now();
            let keep = i64::from(state.config.auth.audit_days) * clock::DAY;
            match state
                .db
                .call(move |c| {
                    Ok(accounts::store::cleanup(c, now)?
                        + accounts::profile::trim_audit(c, now - keep)?)
                })
                .await
            {
                Ok(0) => {}
                Ok(n) => tracing::debug!("removed {n} expired tokens and old log entries"),
                Err(err) => tracing::warn!("cleanup: {err}"),
            }
            let keep_days = state.config.feedback.keep_days;
            match state
                .feedback
                .call(move |c| feedback::cleanup(c, now, keep_days))
                .await
            {
                Ok(0) => {}
                Ok(n) => tracing::debug!("removed {n} old feedback rows"),
                Err(err) => tracing::warn!("feedback cleanup: {err}"),
            }
            match api::files::sweep(&state).await {
                Ok(0) => {}
                Ok(n) => tracing::debug!("deleted {n} files no longer used"),
                Err(err) => tracing::warn!("file cleanup: {err:?}"),
            }
            match api::run_deletions(&state).await {
                Ok(0) => {}
                Ok(n) => tracing::info!("deleted {n} accounts at the end of their grace period"),
                Err(err) => tracing::warn!("scheduled deletions: {err:?}"),
            }
        }
    });
}

/// Prints to stderr, which works before logging is set up, and exits.
/// The configuration, or the reason it cannot be read and how to fix it.
fn load_config(file: Option<&Path>) -> Config {
    Config::load(file).unwrap_or_else(|err| {
        // the file's folder, the working one when no file was named
        let folder = file
            .and_then(Path::parent)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        // absolute: a folder we cannot enter cannot even be looked at as "."
        let folder: PathBuf = std::env::current_dir()
            .map_or_else(|_| folder.to_path_buf(), |cwd| cwd.join(folder))
            .components()
            .filter(|part| !matches!(part, std::path::Component::CurDir))
            .collect();
        fail(
            &secrets::with_owner_hint(&format!("configuration: {err}"), &folder),
            2,
        )
    })
}

/// The data folder made private, the databases and the master key. Data
/// copied from elsewhere may belong to another user, the failure says so.
fn open_stores(config: &Config, config_file: Option<&Path>) -> (Db, Db, keys::MasterKey) {
    let data = config.storage.data_dir.as_path();
    let hinted = |message: &str| secrets::with_owner_hint(message, data);
    if let Err(why) = secrets::prepare(data, config_file) {
        fail(&hinted(&why), 1);
    }
    let db = Db::open(data).unwrap_or_else(|err| fail(&hinted(&err.to_string()), 1));
    let feedback =
        Db::open_feedback(data).unwrap_or_else(|err| fail(&hinted(&format!("feedback: {err}")), 1));
    let master = keys::MasterKey::load(config)
        .unwrap_or_else(|err| fail(&hinted(&format!("master key: {err}")), 2));
    // the files a first start just made, private inside the private folder
    // already, tightened quietly
    if let Err(err) = secrets::tighten(data, None) {
        fail(&format!("data folder permissions: {err}"), 1);
    }
    (db, feedback, master)
}

fn fail(message: &str, code: i32) -> ! {
    eprintln!("{}: {message}", app_brand::SERVER_BINARY);
    std::process::exit(code);
}

async fn shutdown_signal() {
    // a failed signal handler only means we cannot stop gracefully
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
