// SPDX-License-Identifier: AGPL-3.0-or-later

//! SFTP as a backup target, for the server only (the apps leave it out:
//! about 1 MB of SSH). The server's host key is pinned by its SHA-256
//! fingerprint: without one set, connecting fails and says which key the
//! server showed, for the admin to check and set.
//!
//! The repository's targets are blocking calls: this one runs its own
//! small runtime, and is used from the server's blocking tasks.

// used by the instance's backups, which come in the next step
#![cfg_attr(not(test), allow(dead_code))]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_repo::{Error, Listed, Result, Target};

use russh::client::{self, Handle};
use russh::keys::{
    HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate, decode_secret_key,
};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

/// Where the folder is and how to sign in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SftpConfig {
    /// Host name or address.
    pub host: String,
    /// 22 when left at 0.
    pub port: u16,
    /// User name.
    pub username: String,
    /// Password, when no key is given.
    pub password: String,
    /// A private key in OpenSSH format, preferred to a password.
    pub private_key: String,
    /// The key's passphrase, if it has one.
    pub passphrase: String,
    /// The folder, absolute or from the user's home.
    pub path: String,
    /// The server's host key, `SHA256:...` as `ssh-keygen -lf` shows it.
    pub host_key: String,
}

struct Pinned {
    expected: String,
    seen: Arc<Mutex<Option<String>>>,
}

impl client::Handler for Pinned {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> impl Future<Output = std::result::Result<bool, Self::Error>> + Send {
        // a certificate is checked by the key it certifies
        let fingerprint = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.fingerprint(HashAlg::Sha256),
            PublicKeyOrCertificate::Certificate(cert) => {
                PublicKey::from(cert.public_key().clone()).fingerprint(HashAlg::Sha256)
            }
        }
        .to_string();
        let matches = fingerprint == self.expected;
        if let Ok(mut seen) = self.seen.lock() {
            *seen = Some(fingerprint);
        }
        std::future::ready(Ok(matches))
    }
}

struct Connection {
    // kept so the session lives as long as the SFTP channel
    _ssh: Handle<Pinned>,
    sftp: SftpSession,
}

/// An SFTP folder as a target.
pub struct SftpTarget {
    config: SftpConfig,
    /// Taken out when dropped, see [`Drop`].
    runtime: Option<tokio::runtime::Runtime>,
    connection: tokio::sync::Mutex<Option<Connection>>,
}

impl std::fmt::Debug for SftpTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SftpTarget")
            .field("host", &self.config.host)
            .finish_non_exhaustive()
    }
}

fn target_error(what: impl std::fmt::Display) -> Error {
    Error::Target(format!("SFTP: {what}"))
}

fn sftp_error(e: russh_sftp::client::error::Error) -> Error {
    match &e {
        russh_sftp::client::error::Error::Status(status)
            if status.status_code == StatusCode::NoSuchFile =>
        {
            Error::NotFound("repository file")
        }
        _ => target_error(e),
    }
}

impl SftpTarget {
    /// A target for `config`. Nothing is reached until it is used.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for an incomplete config.
    pub fn new(config: SftpConfig) -> Result<Self> {
        if config.host.is_empty() || config.username.is_empty() {
            return Err(Error::Invalid("SFTP needs a host and a user name".into()));
        }
        if config.password.is_empty() && config.private_key.is_empty() {
            return Err(Error::Invalid(
                "SFTP needs a password or a private key".into(),
            ));
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        Ok(Self {
            config,
            runtime: Some(runtime),
            connection: tokio::sync::Mutex::default(),
        })
    }

    fn remote(&self, path: &str) -> String {
        let base = self.config.path.trim_end_matches('/');
        match (base.is_empty(), path.is_empty()) {
            (true, _) => path.to_owned(),
            (false, true) => base.to_owned(),
            (false, false) => format!("{base}/{path}"),
        }
    }

    async fn connect(&self) -> Result<Connection> {
        let seen = Arc::new(Mutex::new(None));
        let handler = Pinned {
            expected: self.config.host_key.trim().to_owned(),
            seen: seen.clone(),
        };
        let ssh_config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(120)),
            ..client::Config::default()
        });
        let port = if self.config.port == 0 {
            22
        } else {
            self.config.port
        };
        let connected = tokio::time::timeout(
            Duration::from_secs(30),
            client::connect(ssh_config, (self.config.host.as_str(), port), handler),
        )
        .await
        .map_err(|_| target_error("the server did not answer"))?;
        let mut ssh = match connected {
            Ok(ssh) => ssh,
            Err(e) => {
                let shown = seen.lock().ok().and_then(|s| s.clone());
                return Err(match shown {
                    Some(key) if key != self.config.host_key.trim() => target_error(format!(
                        "the server's host key is {key}, set it if it is the one you expect"
                    )),
                    _ => target_error(e),
                });
            }
        };
        let signed_in = if self.config.private_key.is_empty() {
            ssh.authenticate_password(&self.config.username, &self.config.password)
                .await
        } else {
            let passphrase =
                (!self.config.passphrase.is_empty()).then_some(self.config.passphrase.as_str());
            let key = decode_secret_key(&self.config.private_key, passphrase)
                .map_err(|e| Error::Invalid(format!("the private key does not read: {e}")))?;
            let hash = ssh.best_supported_rsa_hash().await.ok().flatten().flatten();
            ssh.authenticate_publickey(
                &self.config.username,
                PrivateKeyWithHashAlg::new(Arc::new(key), hash),
            )
            .await
        }
        .map_err(target_error)?;
        if !signed_in.success() {
            return Err(target_error("the user name, password or key was refused"));
        }
        let channel = ssh.channel_open_session().await.map_err(target_error)?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(target_error)?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(target_error)?;
        Ok(Connection { _ssh: ssh, sftp })
    }

    /// Runs `f` on the connection, made or made again as needed. A call
    /// that fails on a connection that broke is tried once more.
    fn with<T>(&self, f: &impl AsyncFn(&SftpSession) -> Result<T>) -> Result<T> {
        let runtime = self.runtime.as_ref().expect("kept until dropped");
        runtime.block_on(async {
            let mut connection = self.connection.lock().await;
            for attempt in 0..2 {
                if connection.is_none() {
                    *connection = Some(self.connect().await?);
                }
                let sftp = &connection.as_ref().expect("made above").sftp;
                match f(sftp).await {
                    Err(Error::Target(_)) if attempt == 0 => *connection = None,
                    other => return other,
                }
            }
            unreachable!("the second attempt returns")
        })
    }

    async fn make_parents(sftp: &SftpSession, path: &str) -> Result<()> {
        let mut so_far = String::new();
        let parts: Vec<&str> = path.split('/').collect();
        for (i, part) in parts[..parts.len().saturating_sub(1)].iter().enumerate() {
            if i > 0 || path.starts_with('/') {
                so_far.push('/');
            }
            so_far.push_str(part);
            if so_far.is_empty() || so_far == "/" {
                continue;
            }
            if !sftp.try_exists(so_far.clone()).await.map_err(sftp_error)? {
                sftp.create_dir(so_far.clone()).await.map_err(sftp_error)?;
            }
        }
        Ok(())
    }
}

impl Drop for SftpTarget {
    fn drop(&mut self) {
        // a target may be dropped by async code (a request's handler): a
        // runtime may not be shut down there, only let go in the background
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// A repository path, nothing above or beside the folder: every call
/// checks it, a damaged repository or a hostile server cannot lead a read
/// or a delete elsewhere.
fn checked(path: &str) -> Result<&str> {
    if app_repo::target::safe_path(path) {
        Ok(path)
    } else {
        Err(Error::Invalid(format!("not a repository path: {path}")))
    }
}

/// How deep and how many entries a listing goes: a repository has a few
/// levels, a server answering with a tree that never ends is cut short.
const MAX_DEPTH: usize = 8;
const MAX_ENTRIES: usize = 4_000_000;

impl Target for SftpTarget {
    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let to = self.remote(checked(path)?);
        let partial = format!("{to}.partial");
        self.with(&async |sftp| {
            Self::make_parents(sftp, &to).await?;
            let mut file = sftp.create(partial.clone()).await.map_err(sftp_error)?;
            file.write_all(bytes).await.map_err(target_error)?;
            file.shutdown().await.map_err(target_error)?;
            drop(file);
            // SFTP version 3 refuses to rename over a file
            if sftp.rename(partial.clone(), to.clone()).await.is_err() {
                let _ = sftp.remove_file(to.clone()).await;
                sftp.rename(partial.clone(), to.clone())
                    .await
                    .map_err(sftp_error)?;
            }
            Ok(())
        })
    }

    fn get(&self, path: &str) -> Result<Vec<u8>> {
        let at = self.remote(checked(path)?);
        self.with(&async |sftp| {
            let mut file = sftp.open(at.clone()).await.map_err(sftp_error)?;
            let mut out = Vec::new();
            (&mut file)
                .take(app_repo::chunk::MAX as u64 * 32)
                .read_to_end(&mut out)
                .await
                .map_err(target_error)?;
            Ok(out)
        })
    }

    fn get_range(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        let at = self.remote(checked(path)?);
        self.with(&async |sftp| {
            let mut file = sftp.open(at.clone()).await.map_err(sftp_error)?;
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(target_error)?;
            let mut out = Vec::new();
            (&mut file)
                .take(len)
                .read_to_end(&mut out)
                .await
                .map_err(target_error)?;
            if out.len() as u64 != len {
                return Err(Error::Corrupt(format!(
                    "{path} is shorter than its index says"
                )));
            }
            Ok(out)
        })
    }

    fn list(&self, prefix: &str) -> Result<Vec<Listed>> {
        let start = prefix.trim_end_matches('/').to_owned();
        let mut out = self.with(&async |sftp| {
            let mut out = Vec::new();
            let mut seen = 0usize;
            let mut folders = vec![(start.clone(), 0usize)];
            while let Some((folder, depth)) = folders.pop() {
                let entries = match sftp.read_dir(self.remote(&folder)).await {
                    Ok(entries) => entries,
                    Err(e) => match sftp_error(e) {
                        Error::NotFound(_) => continue,
                        other => return Err(other),
                    },
                };
                for entry in entries {
                    seen += 1;
                    if seen > MAX_ENTRIES {
                        return Err(Error::Target(
                            "the backup place lists more entries than a repository has".into(),
                        ));
                    }
                    let name = entry.file_name();
                    // a name is one part of a path, never a way out of it
                    if name == "."
                        || name == ".."
                        || name.ends_with(".partial")
                        || name.contains(['/', '\\'])
                    {
                        continue;
                    }
                    let path = if folder.is_empty() {
                        name
                    } else {
                        format!("{folder}/{name}")
                    };
                    if entry.file_type().is_dir() {
                        if depth < MAX_DEPTH {
                            folders.push((path, depth + 1));
                        }
                    } else {
                        out.push(Listed {
                            path,
                            size: entry.metadata().len(),
                        });
                    }
                }
            }
            Ok(out)
        })?;
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn delete(&self, path: &str) -> Result<()> {
        let at = self.remote(checked(path)?);
        self.with(&async |sftp| match sftp.remove_file(at.clone()).await {
            Ok(()) => Ok(()),
            Err(e) => match sftp_error(e) {
                Error::NotFound(_) => Ok(()),
                other => Err(other),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_repository_paths_are_read_or_deleted() {
        assert!(checked("data/ab/abcdef").is_ok());
        for bad in ["../etc/passwd", "/etc/passwd", "data/../../x", ""] {
            assert!(checked(bad).is_err(), "{bad}");
        }
    }

    /// Against a real server when one is given:
    /// `REPO_TEST_SFTP=host,port,user,password` (rclone serve sftp).
    #[test]
    fn against_a_real_server() {
        let Ok(spec) = std::env::var("REPO_TEST_SFTP") else {
            return;
        };
        let p: Vec<&str> = spec.split(',').collect();
        let mut config = SftpConfig {
            host: p[0].into(),
            port: p[1].parse().unwrap(),
            username: p[2].into(),
            password: p[3].into(),
            path: format!("{} backups", app_repo::testing::unique()),
            ..SftpConfig::default()
        };
        // no host key yet: refused, naming the key the server showed
        let unknown = SftpTarget::new(config.clone()).unwrap();
        let Err(Error::Target(message)) = unknown.list("") else {
            panic!("an unknown host key must be refused");
        };
        let key = message
            .split_whitespace()
            .find(|w| w.starts_with("SHA256:"))
            .unwrap_or_else(|| panic!("{message}"))
            .trim_end_matches(',')
            .to_owned();
        config.host_key = key;
        let target = SftpTarget::new(config.clone()).unwrap();
        app_repo::testing::target_calls(&target);
        let life: Arc<dyn Target> = Arc::new(SftpTarget::new(config.clone()).unwrap());
        app_repo::testing::repository_life(&life);
        let wrong = SftpTarget::new(SftpConfig {
            password: "wrong".into(),
            ..config
        })
        .unwrap();
        assert!(matches!(wrong.list(""), Err(Error::Target(m)) if m.contains("refused")));
    }

    /// With a key: `REPO_TEST_SFTP_KEY=host,port,user,key file,host key`.
    #[test]
    fn with_a_private_key() {
        let Ok(spec) = std::env::var("REPO_TEST_SFTP_KEY") else {
            return;
        };
        let p: Vec<&str> = spec.split(',').collect();
        let config = SftpConfig {
            host: p[0].into(),
            port: p[1].parse().unwrap(),
            username: p[2].into(),
            private_key: std::fs::read_to_string(p[3]).unwrap(),
            host_key: p[4].into(),
            path: app_repo::testing::unique(),
            ..SftpConfig::default()
        };
        app_repo::testing::target_calls(&SftpTarget::new(config.clone()).unwrap());
        // through connections that break, with `REPO_TEST_CUT=1`
        if std::env::var("REPO_TEST_CUT").is_ok() {
            let upstream = format!("{}:{}", config.host, config.port).parse().unwrap();
            let proxy = app_repo::testing::cutting_proxy(upstream, 35, 3);
            let folder = format!("{} cut", app_repo::testing::unique());
            let cut: Arc<dyn Target> = Arc::new(
                SftpTarget::new(SftpConfig {
                    host: proxy.ip().to_string(),
                    port: proxy.port(),
                    path: folder.clone(),
                    ..config.clone()
                })
                .unwrap(),
            );
            let direct: Arc<dyn Target> = Arc::new(
                SftpTarget::new(SftpConfig {
                    path: folder,
                    ..config
                })
                .unwrap(),
            );
            app_repo::testing::survives_cuts(&cut, &direct);
        }
    }
}
