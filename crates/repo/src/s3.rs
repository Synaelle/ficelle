// SPDX-License-Identifier: AGPL-3.0-or-later

//! S3 and the services that speak it (AWS, Backblaze B2, Wasabi, `MinIO`,
//! Cloudflare R2...): objects in a bucket under a prefix, every request
//! signed with Signature Version 4.

use std::fmt::Write as _;

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::http::{agent, amz_date, element, elements, encode, now_seconds, read, unreachable};
use crate::target::{Listed, Target, safe_path};

/// The hash of an empty body.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Where and how to reach a bucket.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct S3Config {
    /// The service's address, `https://s3.eu-central-003.backblazeb2.com`
    /// for example. Empty: AWS in `region`.
    pub endpoint: String,
    /// The region, `us-east-1` when the service has none.
    pub region: String,
    /// The bucket.
    pub bucket: String,
    /// A folder in the bucket, `ficelle/laptop` for example. May be empty.
    pub prefix: String,
    /// Access key id.
    pub access_key: String,
    /// Secret access key.
    pub secret_key: String,
    /// The bucket in the path (`endpoint/bucket/key`) rather than in the
    /// host name: what `MinIO` and most self-hosted services want.
    pub path_style: bool,
}

/// A bucket as a target.
pub struct S3Target {
    config: S3Config,
    agent: ureq::Agent,
    /// Seconds since 1970, fixed in tests.
    clock: fn() -> i64,
}

impl std::fmt::Debug for S3Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Target")
            .field("endpoint", &self.config.endpoint)
            .field("bucket", &self.config.bucket)
            .finish_non_exhaustive()
    }
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256>>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(bytes: &[u8]) -> String {
    crate::hex(&Sha256::digest(bytes))
}

/// One request to sign: what Signature Version 4 covers.
pub(crate) struct Request<'a> {
    pub method: &'a str,
    pub host: &'a str,
    /// Already encoded, starting with `/`.
    pub path: &'a str,
    /// Decoded names and values, encoded and sorted when signing.
    pub query: &'a [(&'a str, &'a str)],
    /// Lower case names, other than host and the `x-amz-` ones made here.
    pub headers: &'a [(&'a str, &'a str)],
    pub payload_sha256: &'a str,
}

/// The query string as signed and sent.
pub(crate) fn canonical_query(query: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (encode(k, false), encode(v, false)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// The headers to send, `authorization` included.
pub(crate) fn sign(
    request: &Request<'_>,
    region: &str,
    access_key: &str,
    secret_key: &str,
    seconds: i64,
) -> Vec<(String, String)> {
    let date_time = amz_date(seconds);
    let date = &date_time[..8];
    let mut headers: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.trim().to_owned()))
        .collect();
    headers.push(("host".into(), request.host.to_owned()));
    headers.push((
        "x-amz-content-sha256".into(),
        request.payload_sha256.to_owned(),
    ));
    headers.push(("x-amz-date".into(), date_time.clone()));
    headers.sort();
    let signed = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let mut canonical = String::new();
    let _ = write!(
        canonical,
        "{}\n{}\n{}\n",
        request.method,
        request.path,
        canonical_query(request.query)
    );
    for (k, v) in &headers {
        let _ = writeln!(canonical, "{k}:{v}");
    }
    let _ = write!(canonical, "\n{signed}\n{}", request.payload_sha256);
    let scope = format!("{date}/{region}/s3/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{date_time}\n{scope}\n{}",
        sha256_hex(canonical.as_bytes())
    );
    let key = hmac(
        &hmac(
            &hmac(
                &hmac(format!("AWS4{secret_key}").as_bytes(), date.as_bytes()),
                region.as_bytes(),
            ),
            b"s3",
        ),
        b"aws4_request",
    );
    let signature = crate::hex(&hmac(&key, to_sign.as_bytes()));
    headers.push((
        "authorization".into(),
        format!(
            "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed}, Signature={signature}"
        ),
    ));
    // host is set by the client from the address
    headers.retain(|(k, _)| k != "host");
    headers
}

impl S3Target {
    /// A target for `config`.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for an incomplete config.
    pub fn new(config: S3Config) -> Result<Self> {
        if config.bucket.is_empty() || config.access_key.is_empty() || config.secret_key.is_empty()
        {
            return Err(Error::Invalid(
                "S3 needs a bucket, an access key and a secret key".into(),
            ));
        }
        if !config.endpoint.is_empty() {
            crate::http::place_allowed(&config.endpoint, "S3")?;
        }
        Ok(Self {
            config,
            agent: agent(),
            clock: now_seconds,
        })
    }

    fn region(&self) -> &str {
        if self.config.region.is_empty() {
            "us-east-1"
        } else {
            &self.config.region
        }
    }

    /// The scheme and host, and the path to a key (encoded).
    fn address(&self, key: &str) -> (String, String, String) {
        let (scheme, host) = if self.config.endpoint.is_empty() {
            ("https", format!("s3.{}.amazonaws.com", self.region()))
        } else {
            let (scheme, rest) = self
                .config
                .endpoint
                .split_once("://")
                .unwrap_or(("https", &self.config.endpoint));
            (scheme, rest.trim_end_matches('/').to_owned())
        };
        let bucket = encode(&self.config.bucket, false);
        let key = encode(key, true);
        if self.config.path_style || self.config.endpoint.starts_with("http://") {
            (scheme.to_owned(), host, format!("/{bucket}/{key}"))
        } else {
            (
                scheme.to_owned(),
                format!("{bucket}.{host}"),
                format!("/{key}"),
            )
        }
    }

    fn key(&self, path: &str) -> String {
        let prefix = self.config.prefix.trim_matches('/');
        if prefix.is_empty() {
            path.to_owned()
        } else {
            format!("{prefix}/{path}")
        }
    }

    fn call(
        &self,
        method: &str,
        key: &str,
        query: &[(&str, &str)],
        extra: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, Vec<u8>)> {
        let (scheme, host, path) = self.address(key);
        let payload = if body.is_empty() {
            EMPTY_SHA256.to_owned()
        } else {
            sha256_hex(body)
        };
        let headers = sign(
            &Request {
                method,
                host: &host,
                path: &path,
                query,
                headers: extra,
                payload_sha256: &payload,
            },
            self.region(),
            &self.config.access_key,
            &self.config.secret_key,
            (self.clock)(),
        );
        let query = canonical_query(query);
        let url = if query.is_empty() {
            format!("{scheme}://{host}{path}")
        } else {
            format!("{scheme}://{host}{path}?{query}")
        };
        let result = match method {
            "PUT" => {
                let mut request = self.agent.put(&url);
                for (k, v) in &headers {
                    request = request.header(k, v);
                }
                request.send(body)
            }
            "DELETE" => {
                let mut request = self.agent.delete(&url);
                for (k, v) in &headers {
                    request = request.header(k, v);
                }
                request.call()
            }
            _ => {
                let mut request = self.agent.get(&url);
                for (k, v) in &headers {
                    request = request.header(k, v);
                }
                request.call()
            }
        };
        read(result.map_err(|e| unreachable("S3", &e))?)
    }

    fn refused(status: u16, body: &[u8]) -> Error {
        let text = String::from_utf8_lossy(body);
        let code = element(&text, "Code").unwrap_or_default();
        let message = element(&text, "Message").unwrap_or_default();
        Error::Target(
            format!("S3 answered {status} {code}: {message}")
                .trim_end_matches([':', ' '])
                .to_owned(),
        )
    }
}

impl Target for S3Target {
    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        if !safe_path(path) {
            return Err(Error::Invalid(format!("not a repository path: {path}")));
        }
        let length = bytes.len().to_string();
        match self.call(
            "PUT",
            &self.key(path),
            &[],
            &[("content-length", &length)],
            bytes,
        )? {
            (200..=299, _) => Ok(()),
            (status, body) => Err(Self::refused(status, &body)),
        }
    }

    fn get(&self, path: &str) -> Result<Vec<u8>> {
        match self.call("GET", &self.key(path), &[], &[], &[])? {
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
        match self.call("GET", &self.key(path), &[], &[("range", &range)], &[])? {
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
        let full = self.key(prefix);
        let strip = self.key("");
        let mut out = Vec::new();
        let mut token = String::new();
        loop {
            let mut query = vec![("list-type", "2"), ("prefix", full.as_str())];
            if !token.is_empty() {
                query.push(("continuation-token", token.as_str()));
            }
            let (status, body) = self.call("GET", "", &query, &[], &[])?;
            if status != 200 {
                return Err(Self::refused(status, &body));
            }
            let text = String::from_utf8_lossy(&body);
            for contents in elements(&text, "Contents") {
                let key = element(contents, "Key").unwrap_or_default();
                let size = element(contents, "Size")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                if let Some(path) = key.strip_prefix(&strip) {
                    out.push(Listed {
                        path: path.to_owned(),
                        size,
                    });
                }
            }
            match element(&text, "NextContinuationToken") {
                Some(next) if element(&text, "IsTruncated").as_deref() == Some("true") => {
                    token = next;
                }
                _ => break,
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn delete(&self, path: &str) -> Result<()> {
        match self.call("DELETE", &self.key(path), &[], &[], &[])? {
            (200..=299 | 404, _) => Ok(()),
            (status, body) => Err(Self::refused(status, &body)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The "GET Object" example of the S3 documentation (Signature Version
    /// 4, header based): its signature is published.
    #[test]
    fn signs_the_documented_example() {
        let headers = sign(
            &Request {
                method: "GET",
                host: "examplebucket.s3.amazonaws.com",
                path: "/test.txt",
                query: &[],
                headers: &[("range", "bytes=0-9")],
                payload_sha256: EMPTY_SHA256,
            },
            "us-east-1",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            1_369_353_600,
        );
        let auth = &headers
            .iter()
            .find(|(k, _)| k == "authorization")
            .unwrap()
            .1;
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn addresses_follow_the_style() {
        let mut config = S3Config {
            bucket: "my bucket".into(),
            access_key: "a".into(),
            secret_key: "s".into(),
            region: "eu-west-3".into(),
            prefix: "/ficelle/laptop/".into(),
            ..S3Config::default()
        };
        let aws = S3Target::new(config.clone()).unwrap();
        assert_eq!(
            aws.address(&aws.key("data/ab/cd")),
            (
                "https".into(),
                "my%20bucket.s3.eu-west-3.amazonaws.com".into(),
                "/ficelle/laptop/data/ab/cd".into()
            )
        );
        config.endpoint = "http://127.0.0.1:9000/".into();
        let local = S3Target::new(config.clone()).unwrap();
        assert_eq!(
            local.address("x"),
            (
                "http".into(),
                "127.0.0.1:9000".into(),
                "/my%20bucket/x".into()
            )
        );
        config.endpoint = "ftp://x".into();
        assert!(S3Target::new(config).is_err());
    }
}
