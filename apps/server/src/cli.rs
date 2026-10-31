// SPDX-License-Identifier: AGPL-3.0-or-later

//! Commands for whoever runs the server, on its machine or with
//! `docker exec`: `setup` writes a config file, `admin create` and `admin
//! reset` work on the database (the server may be running, SQLite lets both
//! write), `healthcheck` asks the running server whether it answers.
//!
//! An admin made or reset here has a password alone. It signs in, then must
//! add a passkey or an authenticator app before any admin page answers it.

use std::fmt::Write as _;
use std::io::{BufRead, IsTerminal, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use app_protocol::account::Role;

use crate::accounts::password::{self, Passwords};
use crate::accounts::store::{self, Event};
use crate::accounts::{display_name, factors, normal_username};
use crate::config::Config;
use crate::db::Db;
use crate::setup;

/// Who made an admin, for the security log.
#[derive(Debug, Clone, Copy)]
pub enum Origin {
    /// `admin create`.
    Command,
    /// `[setup] admin` at the first start.
    Automated,
}

impl Origin {
    fn text(self) -> &'static str {
        match self {
            Self::Command => "command line",
            Self::Automated => "automated setup",
        }
    }
}

/// An admin with a password alone, who adds a second factor at the first
/// sign-in. The first one locks setup. Returns whether it did.
///
/// # Errors
///
/// A bad username, a weak password, a taken username, the database.
pub async fn create_admin(
    db: &Db,
    config: &Config,
    username: &str,
    display: Option<&str>,
    password: &str,
    origin: Origin,
) -> Result<bool, String> {
    let username = normal_username(username).ok_or(
        "usernames are 3 to 32 of a-z, 0-9, dot, dash and underscore, starting with a letter or digit",
    )?;
    if let Some(why) = password::weakness(password, &username) {
        return Err(format!("the password is too weak: {why}"));
    }
    let display = display_name(display, &username);
    let passwords = Passwords::new(&config.auth);
    let password = password.to_owned();
    let hash = tokio::task::spawn_blocking(move || passwords.hash(&password))
        .await
        .map_err(|e| e.to_string())??;
    let now = crate::clock::Clock::default().now();
    let made = db
        .call(move |c| {
            let tx = c.transaction()?;
            let Some(user) = store::insert_user(&tx, &username, &display, &hash, Role::Admin, now)?
            else {
                return Ok(None);
            };
            store::set_factor_required(&tx, &user.id, factors::admins_need_second_step(&tx)?)?;
            let first = !setup::is_locked(&tx)?;
            let event = |name| Event {
                name,
                user_id: Some(&user.id),
                detail: Some(origin.text()),
                ..Event::default()
            };
            if first {
                setup::lock(&tx)?;
                store::audit(&tx, now, &event("setup_done"))?;
            }
            store::audit(&tx, now, &event("admin_created"))?;
            tx.commit()?;
            Ok(Some(first))
        })
        .await
        .map_err(|e| e.to_string())?;
    made.ok_or_else(|| "this username is taken".into())
}

/// A new password for an admin locked out: printed once, every second
/// factor removed (a new one is due at the next sign-in), the sign-in lock
/// lifted, every device signed out. Returns the password.
///
/// # Errors
///
/// No such admin, an end-to-end encrypted account (its password cannot be
/// replaced, only its recovery code opens it), the database.
pub async fn reset_admin(db: &Db, config: &Config, username: &str) -> Result<String, String> {
    let name = username.trim().to_ascii_lowercase();
    let lookup = name.clone();
    let user = db
        .call(move |c| store::user_by_name(c, &lookup))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("there is no account {name}"))?;
    if user.role != Role::Admin {
        return Err(format!(
            "{name} is not an admin: an admin resets other people's passwords on the admin page"
        ));
    }
    if user.kind == app_protocol::e2ee::AccountKind::E2ee {
        return Err(format!(
            "{name} is end-to-end encrypted: nobody can replace its password, its recovery code opens it at /recover"
        ));
    }
    let password = new_password();
    let passwords = Passwords::new(&config.auth);
    let given = password.clone();
    let hash = tokio::task::spawn_blocking(move || passwords.hash(&given))
        .await
        .map_err(|e| e.to_string())??;
    let now = crate::clock::Clock::default().now();
    let id = user.id;
    db.call(move |c| {
        let tx = c.transaction()?;
        store::set_password(&tx, &id, &hash)?;
        factors::reset_second_factors(&tx, &id)?;
        store::set_factor_required(&tx, &id, factors::admins_need_second_step(&tx)?)?;
        store::record_success(&tx, &id)?;
        store::revoke_other_devices(&tx, &id, None, now)?;
        store::audit(
            &tx,
            now,
            &Event {
                name: "admin_reset",
                user_id: Some(&id),
                detail: Some("command line"),
                ..Event::default()
            },
        )?;
        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(password)
}

/// Five groups of five from an alphabet without look-alikes, about 120 bits.
fn new_password() -> String {
    const ALPHABET: &[u8; 31] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut bytes = [0u8; 25];
    getrandom::fill(&mut bytes).expect("the system random source works");
    let chars: Vec<char> = bytes
        .iter()
        .map(|b| char::from(ALPHABET[usize::from(b % 31)]))
        .collect();
    chars
        .chunks(5)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// A password typed at a terminal without showing it, or the first line
/// of the input when it is not a terminal.
fn read_password(prompt: &str) -> Result<String, String> {
    if std::io::stdin().is_terminal() {
        rpassword::prompt_password(prompt).map_err(|e| e.to_string())
    } else {
        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        Ok(line.trim_end_matches(['\r', '\n']).to_owned())
    }
}

/// `admin create <name> [--display-name <name>] [--password-file <file>]`
/// and `admin reset <name>`.
///
/// # Errors
///
/// Bad arguments, or what the command refused, worded for a person.
pub async fn admin_command(words: &[String], config: &Config) -> Result<(), String> {
    let mut rest = words.iter().skip(1);
    let mut name = None;
    let mut display = None;
    let mut file: Option<PathBuf> = None;
    while let Some(word) = rest.next() {
        match word.as_str() {
            "--display-name" => {
                display = Some(rest.next().ok_or("--display-name needs a name")?.clone());
            }
            "--password-file" => {
                file = Some(rest.next().ok_or("--password-file needs a file")?.into());
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown argument {other}, see --help"));
            }
            other => name = Some(other.to_owned()),
        }
    }
    let name = name.ok_or("name the admin, see --help")?;
    let db = Db::open(&config.storage.data_dir).map_err(|e| e.to_string())?;
    match words.first().map(String::as_str) {
        Some("create") => {
            let password = if let Some(file) = file {
                std::fs::read_to_string(&file)
                    .map(|p| p.trim_end_matches(['\r', '\n']).to_owned())
                    .map_err(|e| format!("{}: {e}", file.display()))?
            } else {
                let first = read_password("Password: ")?;
                if std::io::stdin().is_terminal() && read_password("The same again: ")? != first {
                    return Err("the two passwords are different".into());
                }
                first
            };
            let first = create_admin(
                &db,
                config,
                &name,
                display.as_deref(),
                &password,
                Origin::Command,
            )
            .await?;
            println!(
                "admin {} made. At the first sign-in on the admin page, add a passkey or an authenticator app: until then it can do nothing else.",
                name.trim().to_ascii_lowercase()
            );
            if first {
                println!("it is the first admin: setup is locked, the setup link no longer works.");
            }
            Ok(())
        }
        Some("reset") => {
            let password = reset_admin(&db, config, &name).await?;
            println!(
                "new password for {}, shown once:",
                name.trim().to_ascii_lowercase()
            );
            println!();
            println!("    {password}");
            println!();
            println!(
                "Its passkeys and authenticator app are removed and every device is signed out. Sign in on the admin page, add a second factor, then change the password."
            );
            Ok(())
        }
        _ => Err("admin create <name> or admin reset <name>, see --help".into()),
    }
}

/// Asks one question on the terminal, `default` when the answer is empty.
fn ask(question: &str, default: &str) -> Result<String, String> {
    if default.is_empty() {
        print!("{question}: ");
    } else {
        print!("{question} [{default}]: ");
    }
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    let read = std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if read == 0 {
        return Err("the input ended".into());
    }
    let answer = line.trim();
    Ok(if answer.is_empty() { default } else { answer }.to_owned())
}

fn yes(question: &str, default: bool) -> Result<bool, String> {
    let answer = ask(question, if default { "yes" } else { "no" })?;
    Ok(matches!(
        answer.to_ascii_lowercase().as_str(),
        "y" | "yes" | "o" | "oui"
    ))
}

/// A toml string, quoted and escaped.
fn quoted(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

/// `setup [--force]`: asks the usual questions and writes the config file
/// (`--config`, or `ficelle.toml` here), readable by its owner only. The
/// answers are checked as the server would load them before anything is
/// written.
///
/// # Errors
///
/// The file exists without `--force`, an answer that does not work, the
/// file cannot be written.
pub fn setup_command(words: &[String], target: &Path) -> Result<(), String> {
    let force = words.iter().any(|w| w == "--force");
    if target.exists() && !force {
        return Err(format!(
            "{} exists, add --force to replace it",
            target.display()
        ));
    }
    println!(
        "This writes {}. Press Enter to keep the value in brackets.",
        target.display()
    );
    println!();
    let defaults = Config::default();
    let public_url = ask(
        "Address people reach the server at, for example https://notes.example.org",
        "",
    )?;
    let bind = ask(
        "Address the server listens on (keep the loopback one behind a reverse proxy)",
        &defaults.server.bind.to_string(),
    )?;
    let data_dir = ask(
        "Data folder (databases, master key, files)",
        &defaults.storage.data_dir.display().to_string(),
    )?;
    let mut text = format!(
        "# Written by `{} setup`.\n\n[server]\nbind = {}\npublic_url = {}\n\n[storage]\ndata_dir = {}\n",
        app_brand::SERVER_BINARY,
        quoted(&bind),
        quoted(&public_url),
        quoted(&data_dir)
    );
    if yes("Send mail (address checks, password resets)?", false)? {
        let host = ask("Mail server", "")?;
        let port = ask("Its port (587 for STARTTLS, 465 for TLS)", "587")?;
        let security = ask("starttls, tls or none", "starttls")?;
        let username = ask("Login, empty for none", "")?;
        let password = if username.is_empty() {
            String::new()
        } else {
            read_password("Its password (not shown): ")?
        };
        let from = ask("Sender, for example Notes <notes@example.org>", "")?;
        let port = port
            .parse::<u16>()
            .map_err(|_| format!("{port} is not a port"))?;
        let _ = write!(
            text,
            "\n[smtp]\nhost = {}\nport = {port}\nsecurity = {}\nusername = {}\npassword = {}\nfrom = {}\n",
            quoted(&host),
            quoted(&security),
            quoted(&username),
            quoted(&password),
            quoted(&from)
        );
    }
    Config::from_sources(&text, std::iter::empty()).map_err(|e| e.to_string())?;
    write_private(target, &text).map_err(|e| format!("{}: {e}", target.display()))?;
    println!();
    println!(
        "Written. Start the server: it prints a setup link in its log, open it to make the first admin."
    );
    Ok(())
}

/// Writes a file only its owner reads, replacing it.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(text.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// `healthcheck`: asks the server at its own address for `/health`, for a
/// container's health check (the image has no shell, no curl). Plain http
/// over loopback, a few seconds at most.
///
/// # Errors
///
/// No answer, or anything but `200 ok`.
pub fn healthcheck(config: &Config) -> Result<(), String> {
    let bind = config.server.bind;
    let ip = match bind.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    let addr = SocketAddr::new(ip, bind.port());
    let wait = Duration::from_secs(5);
    let mut stream = TcpStream::connect_timeout(&addr, wait).map_err(|e| format!("{addr}: {e}"))?;
    stream
        .set_read_timeout(Some(wait))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(
            format!(
                "GET {}/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                config.base_path()
            )
            .as_bytes(),
        )
        .map_err(|e| e.to_string())?;
    let mut answer = String::new();
    stream
        .take(4096)
        .read_to_string(&mut answer)
        .map_err(|e| e.to_string())?;
    let status = answer.lines().next().unwrap_or("");
    if status.split_whitespace().nth(1) == Some("200") && answer.trim_end().ends_with("ok") {
        Ok(())
    } else {
        Err(format!("the server answered {status:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: &Path) -> Config {
        let mut config = Config::default();
        config.storage.data_dir = dir.to_path_buf();
        config.auth.argon2_memory_kib = 64;
        config.auth.argon2_iterations = 1;
        config
    }

    #[tokio::test]
    async fn the_first_admin_locks_setup_and_needs_a_factor() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let db = Db::open(dir.path()).unwrap();
        let weak = create_admin(&db, &config, "root", None, "root", Origin::Command).await;
        assert!(weak.unwrap_err().contains("weak"));
        let bad = create_admin(&db, &config, "-x", None, "a long password", Origin::Command);
        assert!(bad.await.is_err());
        assert!(
            create_admin(
                &db,
                &config,
                "Root",
                Some("The root"),
                "a long password",
                Origin::Command
            )
            .await
            .unwrap(),
            "the first locks setup"
        );
        assert!(
            !create_admin(
                &db,
                &config,
                "second",
                None,
                "another long one",
                Origin::Automated
            )
            .await
            .unwrap()
        );
        let taken = create_admin(
            &db,
            &config,
            "root",
            None,
            "a long password",
            Origin::Command,
        );
        assert!(taken.await.unwrap_err().contains("taken"));
        let (root, locked) = db
            .call(|c| {
                Ok((
                    store::user_by_name(c, "root")?.unwrap(),
                    setup::is_locked(c)?,
                ))
            })
            .await
            .unwrap();
        assert!(locked);
        assert_eq!(root.display_name, "The root");
        assert!(root.factor_required && !root.acts_as_admin());
    }

    #[tokio::test]
    async fn a_reset_gives_a_new_password_and_takes_the_factors() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let db = Db::open(dir.path()).unwrap();
        create_admin(
            &db,
            &config,
            "root",
            None,
            "a long password",
            Origin::Command,
        )
        .await
        .unwrap();
        db.call(|c| {
            let id = store::user_by_name(c, "root")?.unwrap().id;
            store::set_factor_required(c, &id, false)?;
            factors::set_totp_pending(
                c,
                &crate::keys::MasterKey::generate(),
                &id,
                "JBSWY3DPEHPK3PXP",
            )?;
            factors::activate_totp(c, &id, 1)?;
            c.execute(
                "UPDATE users SET locked_until = 9999999999999 WHERE id = ?1",
                [&id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        let password = reset_admin(&db, &config, " ROOT ").await.unwrap();
        assert_eq!(password.len(), 29, "{password}");
        let root = db
            .call(|c| Ok(store::user_by_name(c, "root")?.unwrap()))
            .await
            .unwrap();
        assert!(root.factor_required);
        assert!(root.locked_until.is_none());
        let check = Passwords::new(&config.auth).check(&password, Some(&root.password_hash));
        assert_ne!(check, crate::accounts::password::Check::Bad);
        let id = root.id.clone();
        let totp = db.call(move |c| factors::has_second_factor(c, &id)).await;
        assert!(!totp.unwrap());
        assert!(reset_admin(&db, &config, "nobody").await.is_err());
    }

    #[test]
    fn the_config_written_is_private_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ficelle.toml");
        write_private(&file, "[server]\npublic_url = \"https://n.example.org\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(Config::from_sources(&text, std::iter::empty()).is_ok());
        assert!(setup_command(&[], &file).unwrap_err().contains("--force"));
        for text in ["a \"b\"", "it's", r"c:\data", "x\ny"] {
            let back: toml::Table = toml::from_str(&format!("v = {}", quoted(text))).unwrap();
            assert_eq!(back["v"].as_str(), Some(text), "{text}");
        }
    }

    #[tokio::test]
    async fn the_health_check_asks_the_running_server() {
        let mut config = Config::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        config.server.bind = listener.local_addr().unwrap();
        let app = crate::routes::router(crate::routes::AppState::new(
            config.clone(),
            Db::in_memory().unwrap(),
        ))
        .into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(listener, app).await });
        let config = std::sync::Arc::new(config);
        let checked = tokio::task::spawn_blocking(move || healthcheck(&config))
            .await
            .unwrap();
        assert_eq!(checked, Ok(()));
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut gone = Config::default();
        gone.server.bind = closed.local_addr().unwrap();
        drop(closed);
        assert!(healthcheck(&gone).is_err());
    }
}
