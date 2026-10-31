// SPDX-License-Identifier: AGPL-3.0-or-later

//! Boot configuration. Built-in defaults, then the config file
//! ([`app_brand::CONFIG_FILE`]), then environment variables
//! `{ENV_PREFIX}_SECTION__KEY`. A variable ending in `_FILE` reads its value
//! from that file instead, which is how secrets arrive (Docker secrets): a
//! secret never sits in a plain variable.

use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The whole boot configuration.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Network and http.
    pub server: ServerConfig,
    /// Where data lives.
    pub storage: StorageConfig,
    /// Logs.
    pub log: LogConfig,
    /// Accounts and sign-in.
    pub auth: AuthConfig,
    /// Single sign-on with an OpenID Connect provider.
    pub sso: SsoConfig,
    /// Outgoing email, for verification, password resets and notices.
    pub smtp: SmtpConfig,
    /// What the privacy page says about who runs the server.
    pub privacy: PrivacyConfig,
    /// Attachments: the instance's starting policy, before the admin page
    /// changes it.
    pub files: FilesConfig,
    /// The official catalog of themes, when the admin turns it on.
    pub catalog: CatalogConfig,
    /// Feedback and bug reports, received and sent.
    pub feedback: FeedbackConfig,
    /// The first start, before an admin exists.
    pub setup: SetupConfig,
    /// Update notices from the signed release list.
    pub updates: UpdatesConfig,
}

/// `[updates]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct UpdatesConfig {
    /// Whether the server may ever ask. The admin page has its own switch.
    pub check: bool,
    /// The release list, `releases.json`.
    pub url: String,
    /// Hex Ed25519 public key its entries are signed with. Empty: no check.
    pub public_key: String,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self {
            check: true,
            url: app_brand::RELEASES_URL.to_owned(),
            public_key: app_brand::RELEASES_KEY.to_owned(),
        }
    }
}

/// `[setup]`
#[derive(Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SetupConfig {
    /// Lets the setup link work over plain http from anywhere. Anyone between
    /// the browser and the server could read the link and the admin password:
    /// only for a closed network, warned about at each start.
    pub allow_http: bool,
    /// Automated setup: the first start makes this admin, as
    /// `admin create` does, instead of printing a setup link.
    pub admin: String,
    /// Its password, from `SETUP__ADMIN_PASSWORD_FILE` (a Docker secret),
    /// never from a plain variable.
    pub admin_password: String,
    /// Or a file holding it, read at the first start.
    pub admin_password_file: PathBuf,
}

/// The password stays out of debug output, which can end up in logs.
impl fmt::Debug for SetupConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetupConfig")
            .field("allow_http", &self.allow_http)
            .field("admin", &self.admin)
            .field("admin_password", &"***")
            .field("admin_password_file", &self.admin_password_file)
            .finish()
    }
}

impl SetupConfig {
    /// The automated setup admin's password: the key, else the file.
    ///
    /// # Errors
    ///
    /// Neither given, or the file cannot be read.
    pub fn admin_password(&self) -> Result<String, String> {
        if !self.admin_password.is_empty() {
            return Ok(self.admin_password.clone());
        }
        if self.admin_password_file.as_os_str().is_empty() {
            return Err(
                "setup.admin needs a password: set SETUP__ADMIN_PASSWORD_FILE or setup.admin_password_file".into(),
            );
        }
        std::fs::read_to_string(&self.admin_password_file)
            .map(|p| p.trim_end_matches(['\r', '\n']).to_owned())
            .map_err(|e| format!("{}: {e}", self.admin_password_file.display()))
    }
}

/// Keys holding secrets: a plain environment variable is refused for them,
/// `docker inspect` and `/proc` show those to anyone on the machine. The
/// `_FILE` form (a Docker secret) or the config file works.
const SECRET_KEYS: &[(&str, &str)] = &[
    ("smtp", "password"),
    ("sso", "client_secret"),
    ("setup", "admin_password"),
];

/// `[feedback]`: the starting switches, before the admin page changes them,
/// and the limits against floods.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeedbackConfig {
    /// This instance takes reports from anyone's apps.
    pub receive: bool,
    /// Where this instance's people send theirs.
    pub send_to: app_protocol::feedback::SendTo,
    /// Zero bits a report's proof of work needs, more for busy senders.
    pub pow_bits: u8,
    /// Reports an hour from one server's people, or from one address or
    /// install of an app on no server. The admin page can change it.
    pub per_hour: u32,
    /// Reports a day, counted the same way.
    pub per_day: u32,
    /// Reports a day for the whole instance, then none until the next day.
    pub instance_per_day: u32,
    /// Days a report is kept, unless a moderator keeps it.
    pub keep_days: u32,
}

impl Default for FeedbackConfig {
    fn default() -> Self {
        Self {
            receive: false,
            send_to: app_protocol::feedback::SendTo::Main,
            pow_bits: 18,
            per_hour: 20,
            per_day: 100,
            instance_per_day: 500,
            keep_days: 180,
        }
    }
}

/// `[catalog]`: where the official catalog is and the key it is signed
/// with. The brand's by default, another catalog for testing or for a
/// self-hoster who signs their own.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CatalogConfig {
    /// The folder holding `catalog.json`, ending with a slash.
    pub url: String,
    /// Hex Ed25519 public key. Empty: no catalog.
    pub public_key: String,
    /// Hours a fetched catalog is kept before asking again.
    pub refresh_hours: u32,
}

impl Default for CatalogConfig {
    fn default() -> Self {
        Self {
            url: app_brand::CATALOG_URL.to_owned(),
            public_key: app_brand::CATALOG_KEY.to_owned(),
            refresh_hours: 6,
        }
    }
}

/// `[server]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// Address to listen on. Loopback by default, the server is meant to sit
    /// behind a reverse proxy that handles TLS.
    pub bind: SocketAddr,
    /// Address people reach the server at, for links in invites and emails.
    /// Empty until set. An `https` address also turns on HSTS.
    pub public_url: String,
    /// Largest request body in megabytes. Attachments go through chunked
    /// uploads later, this only bounds a single request.
    pub max_body_mb: u32,
    /// Reverse proxies whose `X-Forwarded-For` header is believed, to know
    /// the real address of a client for rate limits and the audit log. An
    /// address, or a range like `172.30.0.0/16`: in Docker the host's proxy
    /// arrives from the network's gateway.
    pub trusted_proxies: Vec<ProxyRange>,
    /// The client's web build (its `dist` folder), for the admin page at
    /// `/admin`. No page when unset.
    pub web_dir: Option<PathBuf>,
    /// The path everything is served under, for a server at
    /// `example.org/notes` rather than its own subdomain: `/notes`. Unset,
    /// the path of `public_url`. Empty when the proxy removes the path
    /// before passing requests on.
    pub base_path: Option<String>,
}

impl ServerConfig {
    /// Whether `ip` is one of the trusted proxies.
    pub fn trusts(&self, ip: IpAddr) -> bool {
        self.trusted_proxies.iter().any(|range| range.contains(ip))
    }
}

/// One trusted proxy address, or a range of them: `10.0.0.2`, `172.30.0.0/16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyRange {
    addr: IpAddr,
    prefix: u8,
}

impl ProxyRange {
    /// Whether `ip` falls in the range. An IPv4 address written as IPv6
    /// (`::ffff:10.0.0.2`, what a dual stack socket reports) counts as IPv4.
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip.to_canonical()) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

impl From<IpAddr> for ProxyRange {
    fn from(addr: IpAddr) -> Self {
        let addr = addr.to_canonical();
        let prefix = if addr.is_ipv4() { 32 } else { 128 };
        Self { addr, prefix }
    }
}

impl std::str::FromStr for ProxyRange {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bad = || format!("{text:?} is not an address or a range like 10.0.0.0/8");
        let (addr, prefix) = match text.trim().split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (text.trim(), None),
        };
        let mut range = Self::from(addr.parse::<IpAddr>().map_err(|_| bad())?);
        if let Some(prefix) = prefix {
            let prefix: u8 = prefix.parse().map_err(|_| bad())?;
            if prefix > range.prefix {
                return Err(bad());
            }
            range.prefix = prefix;
        }
        Ok(range)
    }
}

impl fmt::Display for ProxyRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = if self.addr.is_ipv4() { 32 } else { 128 };
        if self.prefix == whole {
            write!(f, "{}", self.addr)
        } else {
            write!(f, "{}/{}", self.addr, self.prefix)
        }
    }
}

impl<'de> Deserialize<'de> for ProxyRange {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl Serialize for ProxyRange {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// `[storage]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// Folder holding the database and, later, attachments.
    pub data_dir: PathBuf,
    /// File with the master key that encrypts standard accounts' notes at
    /// rest. Empty: `master.key` in `data_dir`, made on first start.
    pub master_key_file: PathBuf,
}

/// `[log]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogConfig {
    /// Filter in `RUST_LOG` syntax, for example `info` or `server=debug,info`.
    pub level: String,
    /// `text` for people, `json` for log collectors.
    pub format: LogFormat,
}

/// `[auth]`
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    /// Argon2id memory per password check, in KiB.
    pub argon2_memory_kib: u32,
    /// Argon2id passes over that memory.
    pub argon2_iterations: u32,
    /// Argon2id lanes.
    pub argon2_parallelism: u32,
    /// Life of an access token.
    pub access_token_minutes: u32,
    /// Life of a refresh token. Each refresh starts a new one, so a device
    /// used at least this often stays signed in.
    pub refresh_token_days: u32,
    /// Failed sign-ins in a row before the account locks.
    pub lockout_attempts: u32,
    /// First lock length, each further lock doubles it.
    pub lockout_minutes: u32,
    /// Longest lock.
    pub lockout_max_minutes: u32,
    /// Sign-in, registration and refresh requests per minute from one address.
    pub rate_limit_per_minute: u32,
    /// Default life of an invite link.
    pub invite_days: u32,
    /// Days the security log keeps an entry, addresses included.
    pub audit_days: u32,
    /// Days between asking to delete an account and its deletion, while it
    /// can still be cancelled. 0 deletes at once.
    pub deletion_grace_days: u32,
}

/// `[smtp]`. Off while `host` is empty: no email at all, so no password
/// reset by email either.
#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SmtpConfig {
    /// Mail server.
    pub host: String,
    /// Its port. 587 for STARTTLS, 465 for TLS.
    pub port: u16,
    /// `starttls`, `tls`, or `none` for a relay on the same machine.
    pub security: SmtpSecurity,
    /// Login, empty for none.
    pub username: String,
    /// Password, best given through `SMTP__PASSWORD_FILE`.
    pub password: String,
    /// Sender, for example `Notes <notes@example.org>`.
    pub from: String,
}

/// How the connection to the mail server is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    /// Plain connection upgraded with STARTTLS, which must succeed.
    Starttls,
    /// TLS from the first byte.
    Tls,
    /// No encryption, only for a relay on the same machine or network.
    None,
}

impl SmtpConfig {
    /// Whether email is set up.
    pub fn enabled(&self) -> bool {
        !self.host.is_empty()
    }
}

/// The password stays out of debug output, which can end up in logs.
impl fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("security", &self.security)
            .field("username", &self.username)
            .field("password", &"***")
            .field("from", &self.from)
            .finish()
    }
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 587,
            security: SmtpSecurity::Starttls,
            username: String::new(),
            password: String::new(),
            from: String::new(),
        }
    }
}

/// `[privacy]`, shown on the privacy page.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrivacyConfig {
    /// Who runs this server: a person, an association, a company.
    pub operator: String,
    /// How to reach them about personal data, an email or an address.
    pub contact: String,
    /// More to say, in plain text, shown as written.
    pub notice: String,
}

/// `[files]`: the starting policy for every account. The admin page can
/// change it for the instance, a group or a person, see `files::policy`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FilesConfig {
    /// Largest file in megabytes, 0 for no limit.
    pub max_file_mb: u32,
    /// Storage per account in megabytes (its files and notes, and the
    /// spaces it shares), 0 for no limit. 1 GB by default: without a limit
    /// one account, or anyone with open sign-up, could fill the disk.
    pub quota_mb: u32,
    /// Types allowed: extensions (`.pdf`) and MIME patterns (`image/*`).
    /// Empty allows every type not blocked.
    pub allowed_types: Vec<String>,
    /// Types refused.
    pub blocked_types: Vec<String>,
    /// Attachments per note, 0 for no limit.
    pub max_files_per_note: u32,
    /// Longest side images are shrunk to by the apps, 0 to keep them as
    /// they are.
    pub image_max_px: u32,
    /// Quality of shrunk images, 1 to 100.
    pub image_quality: u32,
    /// Whether the apps make a small preview of each image.
    pub thumbnails: bool,
    /// Days a removed file is kept before it is deleted.
    pub retention_days: u32,
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            max_file_mb: 100,
            quota_mb: 1024,
            allowed_types: Vec::new(),
            blocked_types: Vec::new(),
            max_files_per_note: 0,
            image_max_px: 0,
            image_quality: 85,
            thumbnails: true,
            retention_days: 30,
        }
    }
}

/// `[sso]`. Off while `issuer` is empty.
#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SsoConfig {
    /// The provider's issuer address, where its discovery document lives.
    pub issuer: String,
    /// This server's client id at the provider.
    pub client_id: String,
    /// Its client secret, best given through `SSO__CLIENT_SECRET_FILE`.
    pub client_secret: String,
    /// Text of the sign-in button.
    pub label: String,
    /// Scopes asked for besides `openid`.
    pub scopes: Vec<String>,
    /// Make an account on a first sign-in. Off, only identities linked to
    /// an existing account sign in.
    pub create_accounts: bool,
}

impl SsoConfig {
    /// Whether single sign-on is set up.
    pub fn enabled(&self) -> bool {
        !self.issuer.is_empty()
    }
}

/// The secret stays out of debug output, which can end up in logs.
impl fmt::Debug for SsoConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SsoConfig")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &"***")
            .field("label", &self.label)
            .field("scopes", &self.scopes)
            .field("create_accounts", &self.create_accounts)
            .finish()
    }
}

impl Default for SsoConfig {
    fn default() -> Self {
        Self {
            issuer: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            label: "Single sign-on".into(),
            scopes: vec!["profile".into(), "email".into()],
            create_accounts: false,
        }
    }
}

/// How log lines are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// One readable line per event.
    Text,
    /// One json object per event.
    Json,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 8080)),
            public_url: String::new(),
            max_body_mb: 16,
            trusted_proxies: vec![
                IpAddr::from([127, 0, 0, 1]).into(),
                IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]).into(),
            ],
            web_dir: None,
            base_path: None,
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        // Argon2id costs from the OWASP password storage cheat sheet
        Self {
            argon2_memory_kib: 19_456,
            argon2_iterations: 2,
            argon2_parallelism: 1,
            access_token_minutes: 15,
            refresh_token_days: 60,
            lockout_attempts: 5,
            lockout_minutes: 1,
            lockout_max_minutes: 60,
            rate_limit_per_minute: 20,
            invite_days: 7,
            audit_days: 365,
            deletion_grace_days: 30,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("data"),
            master_key_file: PathBuf::new(),
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            format: LogFormat::Text,
        }
    }
}

/// Why the configuration could not be loaded, worded for whoever runs the server.
#[derive(Debug)]
pub enum ConfigError {
    /// The config file exists but cannot be read.
    Read(PathBuf, std::io::Error),
    /// The config file is not valid toml or has unknown keys.
    Parse(String),
    /// A `_FILE` variable points at a file that cannot be read.
    SecretFile(String, PathBuf, std::io::Error),
    /// An environment variable has no section or names an unknown key.
    Env(String),
    /// Values that parse but cannot work together.
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(path, err) => write!(f, "cannot read {}: {err}", path.display()),
            Self::Parse(msg) | Self::Invalid(msg) => f.write_str(msg),
            Self::SecretFile(var, path, err) => {
                write!(f, "{var}: cannot read {}: {err}", path.display())
            }
            Self::Env(msg) => write!(f, "environment: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Loads the file, if any, and applies the environment. `explicit` is a
    /// path given on the command line, which must exist. Without it the file
    /// next to the working directory is used when present.
    ///
    /// # Errors
    ///
    /// Fails on an unreadable or invalid file, unknown keys anywhere, or
    /// values that cannot work.
    pub fn load(explicit: Option<&Path>) -> Result<Self, ConfigError> {
        let path =
            explicit.map_or_else(|| PathBuf::from(app_brand::CONFIG_FILE), Path::to_path_buf);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if explicit.is_none() && err.kind() == std::io::ErrorKind::NotFound => {
                String::new()
            }
            Err(err) => return Err(ConfigError::Read(path, err)),
        };
        Self::from_sources(&text, std::env::vars())
    }

    /// The file contents and the environment, applied over the defaults.
    /// Separate from [`Config::load`] so tests pass their own environment.
    ///
    /// # Errors
    ///
    /// See [`Config::load`].
    pub fn from_sources(
        file: &str,
        env: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, ConfigError> {
        let mut table: toml::Table =
            toml::from_str(file).map_err(|e| ConfigError::Parse(format!("config file: {e}")))?;
        let prefix = format!("{}_", app_brand::ENV_PREFIX);
        let mut vars: Vec<(String, String)> = env
            .into_iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .collect();
        // sorted so a FOO and a FOO_FILE pair always resolve the same way
        vars.sort();
        for (name, value) in vars {
            apply_env(&mut table, &name, &name[prefix.len()..], &value)?;
        }
        let config: Self = toml::Value::Table(table)
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError::Parse(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate_server(&self) -> Result<(), ConfigError> {
        let url = &self.server.public_url;
        let web = url.starts_with("https://") || url.starts_with("http://");
        if !url.is_empty() && !web {
            return Err(ConfigError::Invalid(format!(
                "server.public_url must start with https:// or http://, got {url:?}"
            )));
        }
        let base = self.base_path();
        let fine = base.is_empty()
            || (base.starts_with('/')
                && !base.ends_with('/')
                && !base.contains("//")
                && base.chars().all(|c| {
                    c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '~')
                }));
        if !fine {
            return Err(ConfigError::Invalid(format!(
                "server.base_path must look like /notes (letters, digits, - _ . ~, no slash at the end), got {base:?}"
            )));
        }
        if self.server.max_body_mb == 0 {
            return Err(ConfigError::Invalid(
                "server.max_body_mb must be at least 1".into(),
            ));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), ConfigError> {
        self.validate_server()?;
        let auth = &self.auth;
        argon2::Params::new(
            auth.argon2_memory_kib,
            auth.argon2_iterations,
            auth.argon2_parallelism,
            None,
        )
        .map_err(|e| ConfigError::Invalid(format!("auth.argon2_*: {e}")))?;
        for (key, value) in [
            ("auth.access_token_minutes", auth.access_token_minutes),
            ("auth.refresh_token_days", auth.refresh_token_days),
            ("auth.lockout_attempts", auth.lockout_attempts),
            ("auth.rate_limit_per_minute", auth.rate_limit_per_minute),
            ("auth.invite_days", auth.invite_days),
        ] {
            if value == 0 {
                return Err(ConfigError::Invalid(format!("{key} must be at least 1")));
            }
        }
        let sso = &self.sso;
        if sso.enabled() {
            if url::Url::parse(&sso.issuer).is_err() {
                return Err(ConfigError::Invalid(format!(
                    "sso.issuer must be an address, got {:?}",
                    sso.issuer
                )));
            }
            if sso.client_id.is_empty() {
                return Err(ConfigError::Invalid("sso.client_id is needed".into()));
            }
            if self.server.public_url.is_empty() {
                return Err(ConfigError::Invalid(
                    "single sign-on needs server.public_url, the provider sends people back there"
                        .into(),
                ));
            }
        }
        let smtp = &self.smtp;
        if smtp.enabled() {
            if smtp.from.parse::<lettre::message::Mailbox>().is_err() {
                return Err(ConfigError::Invalid(format!(
                    "smtp.from must be an address like \"Notes <notes@example.org>\", got {:?}",
                    smtp.from
                )));
            }
            if self.server.public_url.is_empty() {
                return Err(ConfigError::Invalid(
                    "email needs server.public_url, the links in it lead there".into(),
                ));
            }
        }
        if !(1..=100).contains(&self.files.image_quality) {
            return Err(ConfigError::Invalid(
                "files.image_quality must be between 1 and 100".into(),
            ));
        }
        let feedback = &self.feedback;
        if !(8..=28).contains(&feedback.pow_bits) {
            return Err(ConfigError::Invalid(
                "feedback.pow_bits must be between 8 and 28".into(),
            ));
        }
        for (key, value) in [
            ("feedback.per_hour", feedback.per_hour),
            ("feedback.per_day", feedback.per_day),
            ("feedback.instance_per_day", feedback.instance_per_day),
            ("feedback.keep_days", feedback.keep_days),
        ] {
            if value == 0 {
                return Err(ConfigError::Invalid(format!("{key} must be at least 1")));
            }
        }
        if self.auth.audit_days == 0 {
            return Err(ConfigError::Invalid(
                "auth.audit_days must be at least 1".into(),
            ));
        }
        tracing_subscriber::EnvFilter::try_new(&self.log.level)
            .map_err(|e| ConfigError::Invalid(format!("log.level {:?}: {e}", self.log.level)))?;
        Ok(())
    }

    /// The path everything is served under, empty or `/notes`: the one set,
    /// else the path of `public_url` without its last slash.
    pub fn base_path(&self) -> String {
        if let Some(base) = &self.server.base_path {
            return base.trim_end_matches('/').to_owned();
        }
        url::Url::parse(&self.server.public_url)
            .map(|u| u.path().trim_end_matches('/').to_owned())
            .unwrap_or_default()
    }

    /// Whether responses should carry HSTS: only when people reach the
    /// server over https, browsers would refuse plain http afterwards.
    pub fn hsts(&self) -> bool {
        self.server.public_url.starts_with("https://")
    }
}

/// `SECTION__KEY=value` sets `[section] key`. A `_FILE` suffix on the key
/// reads the value from that file, trimmed of its final newline.
fn apply_env(
    table: &mut toml::Table,
    var: &str,
    name: &str,
    value: &str,
) -> Result<(), ConfigError> {
    let Some((section, key)) = name.split_once("__") else {
        // FICELLE_CONFIG and similar are read elsewhere, not config keys
        return Ok(());
    };
    let section = section.to_ascii_lowercase();
    let mut key = key.to_ascii_lowercase();
    if SECRET_KEYS.contains(&(section.as_str(), key.as_str())) {
        return Err(ConfigError::Env(format!(
            "{var} holds a secret, a plain variable shows it to anyone who can inspect the process: put it in a file and set {var}_FILE"
        )));
    }
    let value = if let Some(stripped) = key.strip_suffix("_file") {
        let path = PathBuf::from(value);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| ConfigError::SecretFile(var.into(), path, e))?;
        key = stripped.into();
        toml::Value::String(content.trim_end_matches(['\r', '\n']).into())
    } else {
        parse_env_value(value)
    };
    let entry = table
        .entry(section.clone())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let toml::Value::Table(section_table) = entry else {
        return Err(ConfigError::Env(format!(
            "{var}: {section} is not a section"
        )));
    };
    section_table.insert(key, value);
    Ok(())
}

/// Numbers and booleans keep their type, `[...]` is a toml list, anything
/// else is a string, so `..._MAX_BODY_MB=32`, `..._BIND=0.0.0.0:8080` and
/// `..._TRUSTED_PROXIES=["10.0.0.2"]` all do what they say.
fn parse_env_value(raw: &str) -> toml::Value {
    if raw.trim_start().starts_with('[')
        && let Ok(mut table) = toml::from_str::<toml::Table>(&format!("v = {raw}"))
        && let Some(list) = table.remove("v")
    {
        return list;
    }
    if let Ok(n) = raw.parse::<i64>() {
        return toml::Value::Integer(n);
    }
    match raw {
        "true" => toml::Value::Boolean(true),
        "false" => toml::Value::Boolean(false),
        _ => toml::Value::String(raw.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    #[test]
    fn defaults_need_nothing() {
        let config = Config::from_sources("", env(&[])).unwrap();
        assert_eq!(config, Config::default());
        assert_eq!(config.server.bind.to_string(), "127.0.0.1:8080");
        assert!(!config.hsts());
    }

    #[test]
    fn the_file_then_the_environment_win_over_defaults() {
        let file = "[server]\nbind = \"0.0.0.0:9000\"\nmax_body_mb = 8\n[storage]\ndata_dir = \"/srv/ficelle\"\n";
        let config = Config::from_sources(
            file,
            env(&[
                ("FICELLE_SERVER__MAX_BODY_MB", "32"),
                ("FICELLE_LOG__FORMAT", "json"),
            ]),
        )
        .unwrap();
        assert_eq!(config.server.bind.to_string(), "0.0.0.0:9000");
        assert_eq!(config.server.max_body_mb, 32, "environment over file");
        assert_eq!(config.storage.data_dir, PathBuf::from("/srv/ficelle"));
        assert_eq!(config.log.format, LogFormat::Json);
    }

    #[test]
    fn a_file_variable_reads_its_value_from_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let secret = dir.path().join("url");
        std::fs::write(&secret, "https://notes.example.org\n").unwrap();
        let config = Config::from_sources(
            "",
            env(&[("FICELLE_SERVER__PUBLIC_URL_FILE", secret.to_str().unwrap())]),
        )
        .unwrap();
        assert_eq!(config.server.public_url, "https://notes.example.org");
        assert!(config.hsts());
    }

    #[test]
    fn unknown_keys_and_bad_values_are_refused_with_their_name() {
        let typo = Config::from_sources("[server]\nbnd = \"x\"\n", env(&[])).unwrap_err();
        assert!(typo.to_string().contains("bnd"), "{typo}");
        let bad =
            Config::from_sources("", env(&[("FICELLE_SERVER__BIND", "nowhere")])).unwrap_err();
        assert!(bad.to_string().contains("bind"), "{bad}");
        let url = Config::from_sources("[server]\npublic_url = \"notes.example.org\"\n", env(&[]))
            .unwrap_err();
        assert!(url.to_string().contains("public_url"), "{url}");
        let missing = Config::from_sources(
            "",
            env(&[("FICELLE_SERVER__PUBLIC_URL_FILE", "/no/such/file")]),
        )
        .unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("FICELLE_SERVER__PUBLIC_URL_FILE"),
            "{missing}"
        );
    }

    #[test]
    fn lists_and_auth_settings_come_from_the_environment() {
        let config = Config::from_sources(
            "",
            env(&[
                ("FICELLE_SERVER__TRUSTED_PROXIES", "[\"10.0.0.2\", \"::1\"]"),
                ("FICELLE_AUTH__LOCKOUT_ATTEMPTS", "3"),
            ]),
        )
        .unwrap();
        assert_eq!(config.server.trusted_proxies.len(), 2);
        assert_eq!(config.auth.lockout_attempts, 3);
        let ranges = Config::from_sources(
            "",
            env(&[(
                "FICELLE_SERVER__TRUSTED_PROXIES",
                "[\"172.30.0.0/16\", \"fd00::/8\"]",
            )]),
        )
        .unwrap();
        assert!(ranges.server.trusts("172.30.4.1".parse().unwrap()));
        assert!(ranges.server.trusts("::ffff:172.30.0.1".parse().unwrap()));
        assert!(ranges.server.trusts("fd12::1".parse().unwrap()));
        assert!(!ranges.server.trusts("172.31.0.1".parse().unwrap()));
        assert!(!ranges.server.trusts("127.0.0.1".parse().unwrap()));
        for bad in ["10.0.0.0/33", "10.0.0/8", "nowhere", "::1/129"] {
            let err = Config::from_sources(
                &format!("[server]\ntrusted_proxies = [\"{bad}\"]\n"),
                env(&[]),
            )
            .unwrap_err();
            assert!(err.to_string().contains("range"), "{bad}: {err}");
        }
        let all: ProxyRange = "0.0.0.0/0".parse().unwrap();
        assert!(all.contains("203.0.113.9".parse().unwrap()));
        assert_eq!(all.to_string(), "0.0.0.0/0");
        assert_eq!(
            "10.0.0.2".parse::<ProxyRange>().unwrap().to_string(),
            "10.0.0.2"
        );
        let weak = Config::from_sources("[auth]\nargon2_memory_kib = 1\n", env(&[])).unwrap_err();
        assert!(weak.to_string().contains("argon2"), "{weak}");
    }

    #[test]
    fn single_sign_on_needs_its_keys_and_hides_its_secret() {
        let dir = tempfile::tempdir().unwrap();
        let secret = dir.path().join("secret");
        std::fs::write(
            &secret, "s3cret
",
        )
        .unwrap();
        let file = "[server]
public_url = \"https://notes.example.org\"
[sso]
issuer = \"https://id.example.org\"
client_id = \"notes\"
";
        let config = Config::from_sources(
            file,
            env(&[("FICELLE_SSO__CLIENT_SECRET_FILE", secret.to_str().unwrap())]),
        )
        .unwrap();
        assert!(config.sso.enabled());
        assert_eq!(config.sso.client_secret, "s3cret");
        assert!(!format!("{config:?}").contains("s3cret"));

        let no_url = Config::from_sources(
            "[sso]
issuer = \"https://id.example.org\"
client_id = \"notes\"
",
            env(&[]),
        )
        .unwrap_err();
        assert!(no_url.to_string().contains("public_url"), "{no_url}");
    }

    #[test]
    fn email_needs_a_sender_and_an_address_and_hides_its_password() {
        let file = "[server]\npublic_url = \"https://notes.example.org\"\n[smtp]\nhost = \"mail.example.org\"\nfrom = \"Notes <notes@example.org>\"\npassword = \"p4ss\"\n";
        let config = Config::from_sources(file, env(&[])).unwrap();
        assert!(config.smtp.enabled());
        assert_eq!(config.smtp.security, SmtpSecurity::Starttls);
        assert!(!format!("{config:?}").contains("p4ss"));
        let bad = Config::from_sources(
            "[server]\npublic_url = \"https://n.example.org\"\n[smtp]\nhost = \"m\"\nfrom = \"nobody\"\n",
            env(&[]),
        )
        .unwrap_err();
        assert!(bad.to_string().contains("smtp.from"), "{bad}");
        let no_url =
            Config::from_sources("[smtp]\nhost = \"m\"\nfrom = \"n@example.org\"\n", env(&[]))
                .unwrap_err();
        assert!(no_url.to_string().contains("public_url"), "{no_url}");
    }

    #[test]
    fn secrets_are_refused_from_plain_variables() {
        for var in [
            "FICELLE_SMTP__PASSWORD",
            "FICELLE_SSO__CLIENT_SECRET",
            "FICELLE_SETUP__ADMIN_PASSWORD",
        ] {
            let refused = Config::from_sources("", env(&[(var, "hunter22")])).unwrap_err();
            assert!(
                refused.to_string().contains(&format!("{var}_FILE")),
                "{refused}"
            );
            assert!(!refused.to_string().contains("hunter22"), "{refused}");
        }
        let dir = tempfile::tempdir().unwrap();
        let secret = dir.path().join("p");
        std::fs::write(
            &secret,
            "hunter22
",
        )
        .unwrap();
        let config = Config::from_sources(
            "[smtp]
password = \"in the file\"
",
            env(&[("FICELLE_SETUP__ALLOW_HTTP", "true")]),
        )
        .unwrap();
        assert_eq!(config.smtp.password, "in the file");
        assert!(config.setup.allow_http);
        let config = Config::from_sources(
            "",
            env(&[("FICELLE_SMTP__PASSWORD_FILE", secret.to_str().unwrap())]),
        )
        .unwrap();
        assert_eq!(config.smtp.password, "hunter22");
        let config = Config::from_sources(
            "",
            env(&[
                ("FICELLE_SETUP__ADMIN", "root"),
                (
                    "FICELLE_SETUP__ADMIN_PASSWORD_FILE",
                    secret.to_str().unwrap(),
                ),
            ]),
        )
        .unwrap();
        assert_eq!(config.setup.admin_password().unwrap(), "hunter22");
        assert!(!format!("{config:?}").contains("hunter22"));
        let config = Config::from_sources(
            &format!(
                "[setup]\nadmin = \"root\"\nadmin_password_file = {:?}\n",
                secret.to_str().unwrap()
            ),
            env(&[]),
        )
        .unwrap();
        assert_eq!(config.setup.admin_password().unwrap(), "hunter22");
        let missing = Config::from_sources("[setup]\nadmin = \"root\"\n", env(&[])).unwrap();
        assert!(missing.setup.admin_password().is_err());
    }

    #[test]
    fn the_base_path_comes_from_the_public_address_or_its_own_key() {
        let at = |file: &str| Config::from_sources(file, env(&[]));
        assert_eq!(at("").unwrap().base_path(), "");
        let url = "[server]\npublic_url = \"https://example.org/notes/\"\n";
        assert_eq!(at(url).unwrap().base_path(), "/notes");
        let root = "[server]\npublic_url = \"https://notes.example.org\"\n";
        assert_eq!(at(root).unwrap().base_path(), "");
        let stripped = format!("{url}base_path = \"\"\n");
        assert_eq!(
            at(&stripped).unwrap().base_path(),
            "",
            "the proxy removes it"
        );
        let deep = "[server]\nbase_path = \"/apps/notes\"\n";
        assert_eq!(at(deep).unwrap().base_path(), "/apps/notes");
        for bad in ["notes", "/a b", "//x", "/x?y", "/x#y"] {
            let file = format!("[server]\nbase_path = {bad:?}\n");
            assert!(
                at(&file).unwrap_err().to_string().contains("base_path"),
                "{bad}"
            );
        }
    }

    #[test]
    fn other_prefixed_variables_are_not_config_keys() {
        let config = Config::from_sources(
            "",
            env(&[("FICELLE_CONFIG", "/etc/x.toml"), ("HOME", "/root")]),
        )
        .unwrap();
        assert_eq!(config, Config::default());
    }
}
