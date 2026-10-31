// SPDX-License-Identifier: AGPL-3.0-or-later

//! Recovery keys: a random code written down at setup that opens the vault
//! when the password is forgotten. It wraps the database key exactly like the
//! password does, so it is as strong as it is well kept.
//!
//! Codes use Crockford base32 (no I, L, O or U), 32 characters in groups of
//! four, 160 random bits. Typing is forgiving: case, spaces and dashes are
//! ignored and the letters people confuse with digits are read as digits.

use zeroize::Zeroizing;

use crate::crypto::random_bytes;
use crate::error::{Error, Result};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const BYTES: usize = 20;
const LENGTH: usize = BYTES * 8 / 5;
const GROUP: usize = 4;

/// A fresh code, grouped for reading, for example `7K2F-QX9M-...`.
///
/// # Errors
///
/// Fails if the system random generator fails.
pub fn generate() -> Result<Zeroizing<String>> {
    let bytes = Zeroizing::new(random_bytes::<BYTES>()?);
    let mut bits: u64 = 0;
    let mut count = 0;
    let mut code = Zeroizing::new(String::with_capacity(LENGTH + LENGTH / GROUP));
    let mut written = 0usize;
    for &byte in bytes.iter() {
        bits = (bits << 8) | u64::from(byte);
        count += 8;
        while count >= 5 {
            count -= 5;
            if written > 0 && written.is_multiple_of(GROUP) {
                code.push('-');
            }
            code.push(char::from(ALPHABET[((bits >> count) & 31) as usize]));
            written += 1;
        }
    }
    Ok(code)
}

/// The code as stored for key wrapping: uppercase, no separators.
///
/// # Errors
///
/// [`Error::Invalid`] when the text cannot be a recovery code.
pub fn normalize(input: &str) -> Result<Zeroizing<String>> {
    let mut out = Zeroizing::new(String::with_capacity(LENGTH));
    for c in input.chars() {
        let c = match c.to_ascii_uppercase() {
            ' ' | '-' | '\t' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        if !ALPHABET.contains(&(c as u8)) || !c.is_ascii() {
            return Err(Error::Invalid("this is not a recovery key".into()));
        }
        out.push(c);
    }
    if out.len() != LENGTH {
        return Err(Error::Invalid("a recovery key has 32 characters".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_codes_are_grouped_and_read_back() {
        let code = generate().unwrap();
        assert_eq!(code.len(), LENGTH + LENGTH / GROUP - 1);
        assert!(code.split('-').all(|group| group.len() == GROUP));
        assert_eq!(normalize(&code).unwrap().len(), LENGTH);
        assert_ne!(*generate().unwrap(), *code);
    }

    #[test]
    fn reading_forgives_case_spaces_and_lookalikes() {
        let code = "0123-4567-89ab-cdef-ghjk-mnpq-rstv-wxyz";
        let typed = "o123 4567 89AB CDEF GHJK MNPQ RSTV WXYZ";
        assert_eq!(*normalize(code).unwrap(), *normalize(typed).unwrap());
        assert_eq!(
            *normalize("i123-4567-89ab-cdef-ghjk-mnpq-rstv-wxyz").unwrap(),
            *normalize("1123-4567-89ab-cdef-ghjk-mnpq-rstv-wxyz").unwrap()
        );
    }

    #[test]
    fn rejects_wrong_length_and_letters() {
        assert!(normalize("0123-4567").is_err());
        assert!(normalize("U123-4567-89ab-cdef-ghjk-mnpq-rstv-wxyz").is_err());
        assert!(normalize("é123-4567-89ab-cdef-ghjk-mnpq-rstv-wxyz").is_err());
    }
}
