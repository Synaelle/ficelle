// SPDX-License-Identifier: AGPL-3.0-or-later

//! Outgoing email: address checks before an address is kept, password
//! resets, deletion notices. Plain text only, in the language of the
//! request that caused it. The texts live in the app's translation files
//! under `mail.`, so they are translated and checked like the rest.
//!
//! Sending happens in the background: a request never waits for the mail
//! server, and never answers faster or slower depending on whether a mail
//! went out, so the timing tells nothing about accounts.

use std::collections::HashMap;
use std::sync::LazyLock;
#[cfg(test)]
use std::sync::{Arc, Mutex};

use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::{Config, SmtpSecurity};

/// Languages mails are written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// English, the default and for now the only one.
    En,
}

impl Lang {
    /// The first language of an `Accept-Language` header that we write,
    /// English otherwise.
    pub fn from_header(header: Option<&str>) -> Self {
        header
            .unwrap_or("")
            .split(',')
            .map(|part| {
                part.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            })
            .find_map(|tag| (tag.split('-').next() == Some("en")).then_some(Self::En))
            .unwrap_or(Self::En)
    }
}

type Texts = HashMap<String, String>;

fn flatten(prefix: &str, table: &toml::Table, out: &mut Texts) {
    for (key, value) in table {
        let key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(inner) => flatten(&key, inner, out),
            toml::Value::String(text) if key.starts_with("mail.") => {
                out.insert(key, text.clone());
            }
            _ => {}
        }
    }
}

fn load(source: &str) -> Texts {
    let table: toml::Table = toml::from_str(source).expect("the locale files are valid toml");
    let mut out = Texts::new();
    flatten("", &table, &mut out);
    out
}

static EN: LazyLock<Texts> =
    LazyLock::new(|| load(include_str!("../../../packages/locales/en.toml")));

/// A mail text with its `{placeholders}` filled.
pub fn tr(lang: Lang, key: &str, vars: &[(&str, &str)]) -> String {
    let texts = match lang {
        Lang::En => &*EN,
    };
    let mut text = texts
        .get(key)
        .or_else(|| EN.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_owned());
    for (name, value) in vars {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// A sent mail, kept by the test mailer.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct Sent {
    /// Recipient.
    pub to: String,
    /// Subject.
    pub subject: String,
    /// Text.
    pub body: String,
}

/// Where mail goes.
#[derive(Clone)]
pub enum Mailer {
    /// No `[smtp]`: nothing is sent and nothing that needs mail is offered.
    Off,
    /// A mail server.
    Smtp {
        /// Connection pool to it.
        transport: AsyncSmtpTransport<Tokio1Executor>,
        /// Sender.
        from: Mailbox,
    },
    /// Tests read what would have been sent.
    #[cfg(test)]
    Memory(Arc<Mutex<Vec<Sent>>>),
}

/// The host tests use to get a [`Mailer::Memory`].
#[cfg(test)]
pub const TEST_HOST: &str = "mail.test.invalid";

impl Mailer {
    /// The mailer for `[smtp]`. Bad settings were refused when the config
    /// loaded, a transport that still cannot be built turns mail off with
    /// a warning rather than stopping the server.
    pub fn new(config: &Config) -> Self {
        let smtp = &config.smtp;
        if !smtp.enabled() {
            return Self::Off;
        }
        #[cfg(test)]
        if smtp.host == TEST_HOST {
            return Self::Memory(Arc::default());
        }
        let Ok(from) = smtp.from.parse::<Mailbox>() else {
            return Self::Off;
        };
        let builder = match smtp.security {
            SmtpSecurity::Starttls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)
            }
            SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host),
            SmtpSecurity::None => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
                &smtp.host,
            )),
        };
        let mut builder = match builder {
            Ok(b) => b.port(smtp.port),
            Err(err) => {
                tracing::warn!("email is off: {err}");
                return Self::Off;
            }
        };
        if !smtp.username.is_empty() {
            builder = builder.credentials(Credentials::new(
                smtp.username.clone(),
                smtp.password.clone(),
            ));
        }
        Self::Smtp {
            transport: builder.build(),
            from,
        }
    }

    /// Whether mail goes anywhere.
    pub fn enabled(&self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Sends in the background. Failures are logged, never shown to the
    /// person who triggered the mail.
    pub fn send(&self, to: &str, subject: String, body: String) {
        match self {
            Self::Off => {}
            Self::Smtp { transport, from } => {
                let Ok(to) = to.parse::<Mailbox>() else {
                    tracing::warn!("mail not sent, bad address");
                    return;
                };
                let message = Message::builder()
                    .from(from.clone())
                    .to(to)
                    .subject(subject)
                    // said outright, or clients may read accents as ascii
                    .header(ContentType::TEXT_PLAIN)
                    .body(body);
                let message = match message {
                    Ok(m) => m,
                    Err(err) => {
                        tracing::warn!("mail not built: {err}");
                        return;
                    }
                };
                let transport = transport.clone();
                tokio::spawn(async move {
                    if let Err(err) = transport.send(message).await {
                        tracing::warn!("mail not sent: {err}");
                    }
                });
            }
            #[cfg(test)]
            Self::Memory(outbox) => outbox.lock().unwrap().push(Sent {
                to: to.into(),
                subject,
                body,
            }),
        }
    }

    /// Sends and waits for the mail server's answer, for a test mail the
    /// admin asked for. The error says what went wrong, worded by the mail
    /// library.
    ///
    /// # Errors
    ///
    /// Mail off, a bad address, or the mail server refusing or out of reach
    /// within 30 seconds.
    pub async fn send_now(&self, to: &str, subject: String, body: String) -> Result<(), String> {
        match self {
            Self::Off => Err("email is not set up".into()),
            Self::Smtp { transport, from } => {
                let to = to.parse::<Mailbox>().map_err(|e| e.to_string())?;
                let message = Message::builder()
                    .from(from.clone())
                    .to(to)
                    .subject(subject)
                    .header(ContentType::TEXT_PLAIN)
                    .body(body)
                    .map_err(|e| e.to_string())?;
                let sent = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    transport.send(message),
                )
                .await
                .map_err(|_| "the mail server did not answer within 30 seconds".to_owned())?;
                sent.map(|_| ()).map_err(|e| e.to_string())
            }
            #[cfg(test)]
            Self::Memory(_) => {
                self.send(to, subject, body);
                Ok(())
            }
        }
    }

    /// What was sent, for tests.
    #[cfg(test)]
    pub fn sent(&self) -> Vec<Sent> {
        match self {
            Self::Memory(outbox) => outbox.lock().unwrap().clone(),
            _ => Vec::new(),
        }
    }
}

/// A plausible email address: one `@`, something on each side, a dot in
/// the domain, nothing that breaks a header. The mail itself is the real
/// check.
pub fn plausible(address: &str) -> Option<String> {
    let address = address.trim();
    let (local, domain) = address.split_once('@')?;
    let ok = !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !address.contains(char::is_whitespace)
        && !address.contains(['<', '>', ',', ';', '"'])
        && address.len() <= 254
        && address.parse::<Mailbox>().is_ok();
    ok.then(|| format!("{local}@{}", domain.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_from_the_header() {
        assert_eq!(Lang::from_header(Some("fr-FR,fr;q=0.9,en;q=0.8")), Lang::En);
        assert_eq!(Lang::from_header(Some("de-DE, en;q=0.5")), Lang::En);
        assert_eq!(Lang::from_header(None), Lang::En);
    }

    #[test]
    fn texts_come_from_the_locales_with_placeholders_filled() {
        let en = tr(Lang::En, "mail.greeting", &[("name", "Zoé")]);
        assert!(en.contains("Zoé"));
        assert_eq!(tr(Lang::En, "mail.nope", &[]), "mail.nope");
    }

    #[test]
    fn addresses() {
        assert_eq!(
            plausible(" Zoe@Example.ORG "),
            Some("Zoe@example.org".into())
        );
        for bad in [
            "zoe",
            "zoe@",
            "@x.org",
            "zoe@x",
            "a b@x.org",
            "z<e@x.org",
            "zoe@.org",
        ] {
            assert_eq!(plausible(bad), None, "{bad}");
        }
    }
}
