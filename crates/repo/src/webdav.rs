// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WebDAV` (Nextcloud, ownCloud, most NAS, Apache, rclone...): files in a
//! folder at an address, with basic authentication. Folders are made as
//! needed, listings walk them one level at a time (servers often refuse
//! more).

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::http::{agent, decode, element, elements, encode, read, unreachable};
use crate::target::{Listed, Target, safe_path};

/// Where the folder is and who may write to it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebdavConfig {
    /// The folder's address, for Nextcloud
    /// `https://cloud.example.com/remote.php/dav/files/<user>/Backups`.
    pub url: String,
    /// User name.
    pub username: String,
    /// Password, or an app password where the service has them.
    pub password: String,
}

/// A `WebDAV` folder as a target.
pub struct WebdavTarget {
    /// The folder's address, ending with `/`.
    base: String,
    /// Its path part, decoded, to recognise its files in listings.
    base_path: String,
    auth: String,
    agent: ureq::Agent,
    /// Folders known to be there, so each is made once.
    made: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl std::fmt::Debug for WebdavTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebdavTarget")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

const PROPFIND: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/></d:prop></d:propfind>"#;

impl WebdavTarget {
    /// A target for `config`.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for an address that is not http or https.
    pub fn new(config: &WebdavConfig) -> Result<Self> {
        let url = config.url.trim();
        crate::http::place_allowed(url, "WebDAV")?;
        let Some((scheme, rest)) = url.split_once("://") else {
            return Err(Error::Invalid(
                "the WebDAV address must start with https://".into(),
            ));
        };
        if scheme != "https" && scheme != "http" {
            return Err(Error::Invalid(
                "the WebDAV address must start with https://".into(),
            ));
        }
        let path = rest.find('/').map_or("/", |at| &rest[at..]);
        let mut base_path = decode(path);
        if !base_path.ends_with('/') {
            base_path.push('/');
        }
        // the path as typed or pasted, encoded or not: encoded once
        let authority = &rest[..rest.len() - path.len()];
        let base = format!(
            "{scheme}://{authority}{}/",
            encode(&decode(path), true).trim_end_matches('/')
        );
        let auth = format!(
            "Basic {}",
            BASE64.encode(format!("{}:{}", config.username, config.password))
        );
        Ok(Self {
            base,
            base_path,
            auth,
            agent: agent(),
            made: std::sync::Mutex::default(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, encode(path, true))
    }

    fn send(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, Vec<u8>)> {
        let method = ureq::http::Method::from_bytes(method.as_bytes())
            .map_err(|_| Error::Invalid(format!("not a method: {method}")))?;
        let mut request = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("authorization", &self.auth);
        for (k, v) in headers {
            request = request.header(*k, *v);
        }
        let request = request
            .body(body.to_vec())
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let response = self
            .agent
            .run(request)
            .map_err(|e| unreachable("WebDAV", &e))?;
        read(response)
    }

    fn refused(status: u16, body: &[u8]) -> Error {
        let text = String::from_utf8_lossy(body);
        let said = element(&text, "message").unwrap_or_default();
        let what = match status {
            401 => "the user name or password is wrong",
            403 => "this folder may not be written to",
            404 => "the folder does not exist",
            507 => "the server is full",
            _ => "",
        };
        Error::Target(
            format!("WebDAV answered {status}, {what} {said}")
                .trim_end_matches([',', ' '])
                .to_owned(),
        )
    }

    /// Makes the folders of `path` not known to be there, from the top:
    /// the repository's folder too, which may not exist yet either.
    fn make_parents(&self, path: &str) -> Result<()> {
        let mut folders = vec![String::new()];
        let mut so_far = String::new();
        let parts: Vec<&str> = path.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            so_far.push_str(part);
            so_far.push('/');
            folders.push(so_far.clone());
        }
        for folder in folders {
            if self.made.lock().is_ok_and(|m| m.contains(&folder)) {
                continue;
            }
            let url = if folder.is_empty() {
                self.base.clone()
            } else {
                self.url(&folder)
            };
            match self.send("MKCOL", &url, &[], &[])? {
                // made, or there already
                (200..=299 | 405, _) => {}
                (status, body) => return Err(Self::refused(status, &body)),
            }
            if let Ok(mut made) = self.made.lock() {
                made.insert(folder);
            }
        }
        Ok(())
    }

    /// The files and folders right inside `folder` (relative, ending `/`).
    fn level(&self, folder: &str) -> Result<Vec<(String, bool, u64)>> {
        let url = if folder.is_empty() {
            self.base.clone()
        } else {
            self.url(folder)
        };
        let (status, body) = self.send(
            "PROPFIND",
            &url,
            &[
                ("depth", "1"),
                ("content-type", "application/xml; charset=utf-8"),
            ],
            PROPFIND.as_bytes(),
        )?;
        match status {
            207 => {}
            404 => return Ok(Vec::new()),
            _ => return Err(Self::refused(status, &body)),
        }
        let text = String::from_utf8_lossy(&body);
        let mut out = Vec::new();
        for response in elements(&text, "response") {
            let Some(href) = element(response, "href") else {
                continue;
            };
            // an absolute address or a path, decoded
            let path = href
                .split_once("://")
                .and_then(|(_, rest)| rest.find('/').map(|at| rest[at..].to_owned()))
                .unwrap_or(href);
            let path = decode(&path);
            let Some(relative) = path.strip_prefix(&self.base_path) else {
                continue;
            };
            let is_folder = !elements(response, "collection").is_empty();
            let relative = relative.trim_end_matches('/').to_owned();
            // the folder itself
            if relative.is_empty() || format!("{relative}/") == folder {
                continue;
            }
            let size = element(response, "getcontentlength")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            out.push((relative, is_folder, size));
        }
        Ok(out)
    }
}

impl Target for WebdavTarget {
    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        if !safe_path(path) {
            return Err(Error::Invalid(format!("not a repository path: {path}")));
        }
        // a server may refuse a file for a missing folder before reading
        // it and close the connection under the upload: folders first
        self.make_parents(path)?;
        let url = self.url(path);
        match self.send("PUT", &url, &[], bytes)? {
            (200..=299, _) => Ok(()),
            // made by us, removed by someone since
            (404 | 409, _) => {
                if let Ok(mut made) = self.made.lock() {
                    made.clear();
                }
                self.make_parents(path)?;
                match self.send("PUT", &url, &[], bytes)? {
                    (200..=299, _) => Ok(()),
                    (status, body) => Err(Self::refused(status, &body)),
                }
            }
            (status, body) => Err(Self::refused(status, &body)),
        }
    }

    fn get(&self, path: &str) -> Result<Vec<u8>> {
        match self.send("GET", &self.url(path), &[], &[])? {
            (200, body) => Ok(body),
            (404, _) => Err(Error::NotFound("repository file")),
            (status, body) => Err(Self::refused(status, &body)),
        }
    }

    fn get_range(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let range = format!("bytes={offset}-{}", offset + len - 1);
        match self.send("GET", &self.url(path), &[("range", &range)], &[])? {
            // some answer a range with 200 and only its bytes
            (200 | 206, body) if body.len() as u64 == len => Ok(body),
            // others ignore ranges and send it all
            (200, body) => usize::try_from(offset)
                .ok()
                .zip(usize::try_from(offset + len).ok())
                .and_then(|(a, b)| body.get(a..b))
                .map(<[u8]>::to_vec)
                .ok_or_else(|| Error::Corrupt(format!("{path} is shorter than its index says"))),
            (206 | 416, _) => Err(Error::Corrupt(format!(
                "{path} is shorter than its index says"
            ))),
            (404, _) => Err(Error::NotFound("repository file")),
            (status, body) => Err(Self::refused(status, &body)),
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<Listed>> {
        let mut out = Vec::new();
        let mut folders = vec![if prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", prefix.trim_end_matches('/'))
        }];
        while let Some(folder) = folders.pop() {
            for (path, is_folder, size) in self.level(&folder)? {
                if is_folder {
                    folders.push(format!("{path}/"));
                } else if path.starts_with(prefix) && !path.ends_with(".partial") {
                    out.push(Listed { path, size });
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn delete(&self, path: &str) -> Result<()> {
        match self.send("DELETE", &self.url(path), &[], &[])? {
            (200..=299 | 404, _) => Ok(()),
            (status, body) => Err(Self::refused(status, &body)),
        }
    }
}
