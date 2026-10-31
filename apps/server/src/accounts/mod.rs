// SPDX-License-Identifier: AGPL-3.0-or-later

//! Accounts and sign-in: passwords, tokens, their storage, and the rate
//! limit in front of the sign-in endpoints.

pub mod e2ee;
pub mod factors;
pub mod password;
pub mod profile;
pub mod ratelimit;
pub mod store;
pub mod token;
pub mod totp;

use app_protocol::account::DeviceInfo;

/// A username as stored: lower case, 3 to 32 of `a-z 0-9 . _ -`, starting
/// with a letter or digit. `None` when it does not fit.
pub fn normal_username(raw: &str) -> Option<String> {
    let name = raw.trim().to_ascii_lowercase();
    let allowed =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-');
    let first_ok = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    ((3..=32).contains(&name.len()) && first_ok && name.chars().all(allowed)).then_some(name)
}

/// The name shown to others: trimmed, without control characters, at most
/// 64 characters, the username when empty.
pub fn display_name(raw: Option<&str>, username: &str) -> String {
    let clean: String = raw
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let clean: String = clean.trim().chars().take(64).collect();
    if clean.is_empty() {
        username.into()
    } else {
        clean
    }
}

/// A username for an account made by single sign-on, from what the
/// provider calls the person: their username, else the start of their
/// email, made to fit the username rules.
pub fn sso_username(username: Option<&str>, email: Option<&str>) -> String {
    let source = username
        .filter(|u| !u.trim().is_empty())
        .or_else(|| email.and_then(|e| e.split('@').next()))
        .unwrap_or("");
    let mut name: String = source
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .skip_while(|c| !c.is_ascii_alphanumeric())
        .take(32)
        .collect();
    if name.len() < 3 {
        name = format!("user{name}");
    }
    normal_username(&name).unwrap_or_else(|| "user".into())
}

/// Device details as stored: a name to recognise it and a known platform.
/// A `replaces` that cannot be a device id is dropped.
pub fn device_info(raw: &DeviceInfo) -> DeviceInfo {
    let name: String = raw.name.chars().filter(|c| !c.is_control()).collect();
    let name = name.trim();
    let platform = raw.platform.trim().to_ascii_lowercase();
    let known = ["android", "ios", "windows", "macos", "linux", "web"];
    DeviceInfo {
        name: if name.is_empty() {
            "Unknown device".into()
        } else {
            name.chars().take(64).collect()
        },
        platform: if known.contains(&platform.as_str()) {
            platform
        } else {
            "other".into()
        },
        replaces: raw
            .replaces
            .as_deref()
            .map(str::trim)
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 64
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            .map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames() {
        assert_eq!(normal_username(" Anna.B "), Some("anna.b".into()));
        assert_eq!(normal_username("ab"), None, "too short");
        assert_eq!(
            normal_username("-anna"),
            None,
            "starts with a letter or digit"
        );
        assert_eq!(normal_username("anna b"), None);
        assert_eq!(normal_username("annà"), None, "ascii only, no look-alikes");
    }

    #[test]
    fn usernames_from_a_provider() {
        assert_eq!(sso_username(Some("Anna.B"), None), "anna.b");
        assert_eq!(
            sso_username(None, Some("Zoé Martin@example.org")),
            "zo--martin"
        );
        assert_eq!(sso_username(Some("  "), Some("jo@x")), "userjo");
        assert_eq!(sso_username(Some("__x"), None), "userx");
        assert_eq!(sso_username(None, None), "user");
        assert_eq!(sso_username(Some(&"a".repeat(50)), None).len(), 32);
    }

    #[test]
    fn display_names_and_devices() {
        assert_eq!(display_name(Some("  "), "anna"), "anna");
        assert_eq!(display_name(Some("Anna\u{7}B"), "anna"), "AnnaB");
        let device = device_info(&DeviceInfo {
            name: " ".into(),
            platform: "Amiga".into(),
            replaces: Some("../x".into()),
        });
        assert_eq!(
            (device.name.as_str(), device.platform.as_str()),
            ("Unknown device", "other")
        );
        assert_eq!(device.replaces, None);
        let again = device_info(&DeviceInfo {
            name: "Chrome, admin page".into(),
            platform: "web".into(),
            replaces: Some(" 0196f0a2-7c1d-7e00-8000-000000000001 ".into()),
        });
        assert_eq!(
            again.replaces.as_deref(),
            Some("0196f0a2-7c1d-7e00-8000-000000000001")
        );
    }
}
