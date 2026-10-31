// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the S3 and `WebDAV` targets share: the HTTP client, reading answers,
//! the little XML they answer with, dates.

use std::io::Read;
use std::time::Duration;

use crate::error::{Error, Result};

/// The most an answer may be: a pack, with room.
pub(crate) const MAX_BODY: u64 = 64 * 1024 * 1024;

/// An agent that hands back error answers instead of failing on them, so
/// the target's own words reach the person.
pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_global(Some(Duration::from_mins(10)))
        .http_status_as_error(false)
        // WebDAV's own: PROPFIND, MKCOL
        .allow_non_standard_methods(true)
        .build()
        .into()
}

/// Whether an S3 or `WebDAV` address may be used: https anywhere but the
/// link-local and cloud metadata addresses,
/// plain http only on this machine or a home network, where a NAS often has
/// no certificate. Names are taken as typed, they are not looked up.
///
/// # Errors
///
/// [`Error::Invalid`] saying why, in words.
pub(crate) fn place_allowed(url: &str, what: &str) -> Result<()> {
    let (scheme, rest) = url
        .trim()
        .split_once("://")
        .ok_or_else(|| Error::Invalid(format!("the {what} address must start with https://")))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_port.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host_port.split(':').next().unwrap_or_default(),
    }
    .to_ascii_lowercase();
    let ip = host.parse::<std::net::IpAddr>().ok();
    let link_local = match ip {
        Some(std::net::IpAddr::V4(v4)) => v4.is_link_local(),
        Some(std::net::IpAddr::V6(v6)) => {
            v6.is_unicast_link_local() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_link_local())
        }
        None => host == "metadata.google.internal",
    };
    if link_local {
        return Err(Error::Invalid(format!(
            "the {what} address is a link-local or cloud metadata address"
        )));
    }
    let ending = host.rsplit_once('.').map(|(_, end)| end);
    let home = host == "localhost"
        || matches!(ending, Some("local" | "localhost"))
        || match ip {
            Some(std::net::IpAddr::V4(v4)) => v4.is_loopback() || v4.is_private(),
            Some(std::net::IpAddr::V6(v6)) => v6.is_loopback() || v6.is_unique_local(),
            None => false,
        };
    match scheme {
        "https" => Ok(()),
        "http" if home => Ok(()),
        "http" => Err(Error::Invalid(format!(
            "the {what} address must start with https://, plain http is only for this machine or a home network"
        ))),
        _ => Err(Error::Invalid(format!(
            "the {what} address must start with https://"
        ))),
    }
}

/// A failure to reach the target, in words.
pub(crate) fn unreachable(what: &str, e: &ureq::Error) -> Error {
    Error::Target(format!("{what} could not be reached: {e}"))
}

/// An answer's status and body, the body read up to [`MAX_BODY`].
pub(crate) fn read(response: ureq::http::Response<ureq::Body>) -> Result<(u16, Vec<u8>)> {
    let status = response.status().as_u16();
    let mut body = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)?;
    if body.len() as u64 > MAX_BODY {
        return Err(Error::Target(
            "the answer is larger than any file here".into(),
        ));
    }
    Ok((status, body))
}

/// Unescapes the five XML entities and numeric ones.
pub(crate) fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(c) = decoded {
            out.push(c);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// The inner text of every element named `local` (any namespace prefix),
/// in order. Elements of that name are not expected inside each other.
pub(crate) fn elements<'a>(xml: &'a str, local: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find('<') {
        rest = &rest[at + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        if tag.starts_with('/') || tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        let name = tag
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or_default();
        let name_local = name.rsplit(':').next().unwrap_or(name);
        if name_local != local {
            continue;
        }
        if tag.ends_with('/') {
            out.push("");
            rest = &rest[end + 1..];
            continue;
        }
        let body = &rest[end + 1..];
        // the closing tag, with the same prefix as the opening one
        let close = format!("</{name}>");
        let Some(stop) = body.find(&close) else { break };
        out.push(&body[..stop]);
        rest = &body[stop + close.len()..];
    }
    out
}

/// The first element named `local`, unescaped.
pub(crate) fn element(xml: &str, local: &str) -> Option<String> {
    elements(xml, local).first().map(|t| unescape(t.trim()))
}

/// Year, month, day of a day count from 1970-01-01.
pub(crate) fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Seconds since 1970 as `20130524T000000Z`.
pub(crate) fn amz_date(seconds: i64) -> String {
    let (y, m, d) = civil(seconds.div_euclid(86_400));
    let s = seconds.rem_euclid(86_400);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        s / 3600,
        s % 3600 / 60,
        s % 60
    )
}

/// Seconds since 1970, now.
pub(crate) fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Percent encodes all but the unreserved characters, and `/` when
/// `keep_slash`.
pub(crate) fn encode(text: &str, keep_slash: bool) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'~')
            || (keep_slash && b == b'/')
        {
            out.push(char::from(b));
        } else {
            // writing to a String cannot fail
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Decodes `%XX` escapes, leaving anything else as it is.
pub(crate) fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_only_at_home_and_never_metadata() {
        for ok in [
            "https://s3.example.org",
            "https://user@dav.example.org:8443/backups",
            "http://192.168.1.20:9000",
            "http://nas.local/dav",
            "http://[fd12::3]:9000",
            "http://localhost:9000",
        ] {
            assert!(place_allowed(ok, "S3").is_ok(), "{ok}");
        }
        for bad in [
            "http://s3.example.org",
            "http://203.0.113.4",
            "https://169.254.169.254/latest",
            "http://169.254.169.254",
            "https://[fe80::1]",
            "http://metadata.google.internal",
            "ftp://nas.local",
            "s3.example.org",
        ] {
            assert!(place_allowed(bad, "S3").is_err(), "{bad}");
        }
    }

    #[test]
    fn xml_is_read_whatever_its_prefixes() {
        let xml = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response><d:href>/a%20b/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response><D:response><D:href>/x&amp;y</D:href><D:getcontentlength>12</D:getcontentlength></D:response></d:multistatus>"#;
        let responses = elements(xml, "response");
        assert_eq!(responses.len(), 2);
        assert_eq!(element(responses[0], "href").as_deref(), Some("/a%20b/"));
        assert_eq!(elements(responses[0], "collection"), vec![""]);
        assert_eq!(element(responses[1], "href").as_deref(), Some("/x&y"));
        assert_eq!(
            element(responses[1], "getcontentlength").as_deref(),
            Some("12")
        );
        assert_eq!(
            unescape("&lt;a&gt; &#233;&#x41; &bogus; &"),
            "<a> éA &bogus; &"
        );
    }

    #[test]
    fn dates_and_escapes() {
        assert_eq!(amz_date(1_369_353_600), "20130524T000000Z");
        assert_eq!(amz_date(951_782_400 + 3661), "20000229T010101Z");
        assert_eq!(encode("a b/c~d+é", true), "a%20b/c~d%2B%C3%A9");
        assert_eq!(encode("a/b", false), "a%2Fb");
        assert_eq!(decode("a%20b%2Fc%C3%A9%zz%"), "a b/cé%zz%");
    }
}
