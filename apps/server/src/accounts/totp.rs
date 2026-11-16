// SPDX-License-Identifier: AGPL-3.0-or-later

//! Authenticator app codes (RFC 6238: HMAC-SHA1, six digits, 30 seconds,
//! what every app supports) and recovery codes.

use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, KeyInit, Mac};
use qrcode::QrCode;
use qrcode::render::svg;
use sha1::Sha1;

use super::token::{self, TokenHash};

/// Length of a step in milliseconds.
const STEP_MS: i64 = 30_000;

/// Recovery codes handed out at a time.
pub const RECOVERY_CODES: usize = 10;

/// A new secret: 160 random bits, base32 as apps expect.
///
/// # Panics
///
/// When the operating system cannot provide random bytes.
pub fn new_secret() -> String {
    let mut bytes = [0u8; 20];
    getrandom::fill(&mut bytes).expect("the system random source works");
    BASE32_NOPAD.encode(&bytes)
}

fn code_at(secret: &[u8], step: i64) -> u32 {
    let mut mac = Hmac::<Sha1>::new_from_slice(secret).expect("hmac takes any key length");
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let value = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    value % 1_000_000
}

/// The step a code typed at `now` belongs to, if it is right. One step
/// either side is accepted for clocks that drift, and only steps after
/// `last_step`, so a code seen over a shoulder cannot be used again.
pub fn check(secret: &str, code: &str, now: i64, last_step: Option<i64>) -> Option<i64> {
    let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let typed: u32 = digits.parse().ok()?;
    let key = BASE32_NOPAD.decode(secret.as_bytes()).ok()?;
    let current = now.div_euclid(STEP_MS);
    (current - 1..=current + 1)
        .filter(|&step| last_step.is_none_or(|last| step > last))
        .find(|&step| code_at(&key, step) == typed)
}

/// The code an app shows at `now`, for tests.
#[cfg(test)]
pub fn code_now(secret: &str, now: i64) -> String {
    let key = BASE32_NOPAD
        .decode(secret.as_bytes())
        .expect("a base32 secret");
    format!("{:06}", code_at(&key, now.div_euclid(STEP_MS)))
}

/// The `otpauth://` link apps read from the QR code.
pub fn uri(secret: &str, issuer: &str, username: &str) -> String {
    let mut url = url::Url::parse("otpauth://totp/").expect("a valid base");
    url.set_path(&format!("{issuer}:{username}"));
    url.query_pairs_mut()
        .append_pair("secret", secret)
        .append_pair("issuer", issuer)
        .append_pair("algorithm", "SHA1")
        .append_pair("digits", "6")
        .append_pair("period", "30");
    url.into()
}

/// The link as a QR code, an svg document the page shows as an image.
pub fn qr_svg(uri: &str) -> String {
    QrCode::new(uri.as_bytes()).map_or_else(
        |_| String::new(),
        |code| {
            code.render::<svg::Color<'_>>()
                .min_dimensions(200, 200)
                .quiet_zone(true)
                .build()
        },
    )
}

/// Six random digits for a code by email.
///
/// # Panics
///
/// When the operating system cannot provide random bytes.
pub fn email_code() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the system random source works");
    // 2^64 is not a multiple of a million, but the bias is below 1e-13
    format!("{:06}", u64::from_le_bytes(bytes) % 1_000_000)
}

/// New recovery codes and their hashes: 12 characters of Crockford base32
/// in groups of four, 60 random bits each.
///
/// # Panics
///
/// When the operating system cannot provide random bytes.
pub fn new_recovery_codes() -> Vec<(String, TokenHash)> {
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    (0..RECOVERY_CODES)
        .map(|_| {
            let mut bytes = [0u8; 12];
            getrandom::fill(&mut bytes).expect("the system random source works");
            let chars: Vec<char> = bytes
                .iter()
                .map(|b| char::from(ALPHABET[usize::from(b & 31)]))
                .collect();
            let code = chars
                .chunks(4)
                .map(|c| c.iter().collect::<String>())
                .collect::<Vec<_>>()
                .join("-");
            let hash = hash_recovery_code(&code);
            (code, hash)
        })
        .collect()
}

/// The stored form of a recovery code: typed in any case, with or without
/// dashes, and with the letters people mistake for digits, it still matches.
pub fn hash_recovery_code(code: &str) -> TokenHash {
    let normal: String = code
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| match c.to_ascii_lowercase() {
            'i' | 'l' => '1',
            'o' => '0',
            c => c,
        })
        .collect();
    token::hash(&normal)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RFC 6238 test secret, "12345678901234567890".
    fn rfc_secret() -> String {
        BASE32_NOPAD.encode(b"12345678901234567890")
    }

    #[test]
    fn matches_the_rfc_test_vectors() {
        // RFC 6238 appendix B, SHA1, last six of the eight digits
        for (seconds, code) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
        ] {
            let now = seconds * 1000;
            assert_eq!(
                check(&rfc_secret(), code, now, None),
                Some(now / STEP_MS),
                "{seconds}"
            );
        }
    }

    #[test]
    fn a_step_of_drift_is_fine_but_a_code_works_once() {
        let secret = rfc_secret();
        let now = 1_111_111_109_000;
        let late = now + STEP_MS;
        let step = check(&secret, "081804", late, None).expect("one step late is accepted");
        assert_eq!(check(&secret, "081804", late, Some(step)), None, "replay");
        assert_eq!(check(&secret, "081804", now + 3 * STEP_MS, None), None);
        assert_eq!(check(&secret, "081 804", now, None), Some(step), "spaces");
        assert_eq!(check(&secret, "08180", now, None), None);
    }

    #[test]
    fn links_and_qr_codes() {
        let link = uri("ABC", "Ficelle Server", "anna");
        assert!(
            link.starts_with(
                "otpauth://totp/Ficelle%20Server:anna?secret=ABC&issuer=Ficelle+Server"
            ),
            "{link}"
        );
        assert!(qr_svg(&link).contains("<svg"));
        assert_eq!(new_secret().len(), 32);
    }

    #[test]
    fn recovery_codes_match_however_they_are_typed() {
        let codes = new_recovery_codes();
        assert_eq!(codes.len(), RECOVERY_CODES);
        let (code, hash) = &codes[0];
        assert_eq!(code.len(), 14);
        assert_eq!(
            hash_recovery_code(&code.to_uppercase().replace('-', " ")),
            *hash
        );
        assert_eq!(hash_recovery_code("ab1o-x"), hash_recovery_code("ABLO-X"));
    }
}
