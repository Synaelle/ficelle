// SPDX-License-Identifier: AGPL-3.0-or-later

//! Recovery codes for accounts, in the same format as a vault's recovery
//! key in the apps: Crockford base32 (no I, L, O or U), 32 characters in
//! groups of four, 160 random bits. Typing is forgiving: case, spaces and
//! dashes are ignored and the letters people confuse with digits are read
//! as digits.

use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::{Error, Result};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const BYTES: usize = 20;
const LENGTH: usize = BYTES * 8 / 5;
const GROUP: usize = 4;

/// A fresh code, grouped for reading, for example `7K2F-QX9M-...`.
pub fn generate() -> Zeroizing<String> {
    let mut bytes = Zeroizing::new([0u8; BYTES]);
    OsRng.fill_bytes(bytes.as_mut());
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
            code.push(char::from(
                ALPHABET[usize::try_from((bits >> count) & 31).unwrap_or(0)],
            ));
            written += 1;
        }
    }
    code
}

/// The code as it goes into key derivation: uppercase, no separators.
///
/// # Errors
///
/// [`Error::RecoveryCode`] when the text cannot be a recovery code.
pub fn normalize(input: &str) -> Result<Zeroizing<String>> {
    let mut out = Zeroizing::new(String::with_capacity(LENGTH));
    for c in input.chars() {
        let c = match c.to_ascii_uppercase() {
            ' ' | '-' | '\t' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        if !c.is_ascii() || !ALPHABET.contains(&(c as u8)) {
            return Err(Error::RecoveryCode);
        }
        out.push(c);
    }
    if out.len() != LENGTH {
        return Err(Error::RecoveryCode);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_grouped_and_read_back_however_typed() {
        let code = generate();
        assert_eq!(code.len(), LENGTH + LENGTH / GROUP - 1);
        assert!(code.split('-').all(|group| group.len() == GROUP));
        let typed = code.to_lowercase().replace('-', " ").replace('0', "o");
        assert_eq!(*normalize(&typed).unwrap(), *normalize(&code).unwrap());
        assert_ne!(*generate(), *code);
        assert_eq!(normalize("0123-4567"), Err(Error::RecoveryCode));
        assert_eq!(
            normalize("U123-4567-89ab-cdef-ghjk-mnpq-rstv-wxyz"),
            Err(Error::RecoveryCode)
        );
    }
}
