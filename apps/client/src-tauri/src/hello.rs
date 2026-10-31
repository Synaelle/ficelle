// SPDX-License-Identifier: AGPL-3.0-or-later

//! Windows Hello as an [`Authenticator`]. Hello keeps a private key in the
//! TPM that only signs after face, fingerprint or PIN. Signing a fixed
//! challenge gives the same bytes every time (RSA PKCS#1 v1.5), which become
//! the key that seals the database key. A new Hello enrolment gives a new
//! private key, so the old sealed key stops opening and the password is
//! asked again.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use app_core::crypto::{open_with_material, seal_with_material};
use app_core::error::{Error, Result};
use app_core::keystore::Authenticator;
use windows::Security::Credentials::{
    KeyCredential, KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
};
use windows::Security::Cryptography::CryptographicBuffer;
use windows::core::{Array, HSTRING};
use winsafe::{AtomStr, HWND};
use zeroize::Zeroizing;

/// Fixed so the signature, and the key made from it, never change.
const CHALLENGE: &[u8] = b"ficelle windows hello unlock v1";

pub struct WindowsHello;

fn failed(err: &windows::core::Error) -> Error {
    Error::KeyStore(format!("windows hello: {err}"))
}

fn credential_name(name: &str) -> HSTRING {
    HSTRING::from(format!("{}.{name}", app_brand::IDENTIFIER))
}

fn check(status: KeyCredentialStatus) -> Result<()> {
    match status {
        KeyCredentialStatus::Success => Ok(()),
        KeyCredentialStatus::UserCanceled => Err(Error::Cancelled),
        KeyCredentialStatus::NotFound => Err(Error::NotRemembered),
        other => Err(Error::KeyStore(format!("windows hello status {}", other.0))),
    }
}

/// The window class of the Windows Hello prompt.
const PROMPT_CLASS: &str = "Credential Dialog Xaml Host";
/// How long to look for the prompt before leaving it where it is.
const PROMPT_WAIT: Duration = Duration::from_secs(5);

/// Runs `call`, which shows the Windows Hello prompt, and brings the prompt
/// to the front once it appears. A desktop app cannot give `KeyCredential`
/// its window, so Windows may open the prompt behind the app; ours is the
/// foreground window (the command focused it), which lets it hand the
/// foreground over.
fn with_prompt_forward<T>(call: impl FnOnce() -> T) -> T {
    let done = Arc::new(AtomicBool::new(false));
    let watching = Arc::clone(&done);
    let watcher = std::thread::spawn(move || {
        let start = Instant::now();
        while !watching.load(Ordering::Relaxed) && start.elapsed() < PROMPT_WAIT {
            if let Ok(Some(prompt)) = HWND::FindWindow(Some(AtomStr::from_str(PROMPT_CLASS)), None)
            {
                prompt.SetForegroundWindow();
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let out = call();
    done.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    out
}

/// Signs the challenge with `credential`, asking the user.
fn sign(credential: &KeyCredential) -> Result<Zeroizing<Vec<u8>>> {
    let challenge = CryptographicBuffer::CreateFromByteArray(CHALLENGE).map_err(|e| failed(&e))?;
    let result = with_prompt_forward(|| {
        credential
            .RequestSignAsync(&challenge)
            .and_then(|op| op.join())
    })
    .map_err(|e| failed(&e))?;
    check(result.Status().map_err(|e| failed(&e))?)?;
    let buffer = result.Result().map_err(|e| failed(&e))?;
    let mut bytes = Array::<u8>::new();
    CryptographicBuffer::CopyToByteArray(&buffer, &mut bytes).map_err(|e| failed(&e))?;
    Ok(Zeroizing::new(bytes.to_vec()))
}

impl Authenticator for WindowsHello {
    fn available(&self) -> bool {
        KeyCredentialManager::IsSupportedAsync()
            .and_then(|op| op.join())
            .unwrap_or(false)
    }

    fn seal(&self, name: &str, secret: &[u8]) -> Result<Vec<u8>> {
        let result = with_prompt_forward(|| {
            KeyCredentialManager::RequestCreateAsync(
                &credential_name(name),
                KeyCredentialCreationOption::ReplaceExisting,
            )
            .and_then(|op| op.join())
        })
        .map_err(|e| failed(&e))?;
        check(result.Status().map_err(|e| failed(&e))?)?;
        let credential = result.Credential().map_err(|e| failed(&e))?;
        // creating already asked the user, signing may ask once more
        let material = sign(&credential)?;
        seal_with_material(&material, secret)
    }

    fn open(&self, name: &str, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let result = KeyCredentialManager::OpenAsync(&credential_name(name))
            .and_then(|op| op.join())
            .map_err(|e| failed(&e))?;
        check(result.Status().map_err(|e| failed(&e))?)?;
        let credential = result.Credential().map_err(|e| failed(&e))?;
        open_with_material(&sign(&credential)?, sealed)
    }

    fn remove(&self, name: &str) -> Result<()> {
        match KeyCredentialManager::DeleteAsync(&credential_name(name)).and_then(|op| op.join()) {
            Ok(()) => Ok(()),
            // a missing credential is already removed
            Err(err) if err.code().0.cast_unsigned() == 0x8009_0011 => Ok(()),
            Err(err) => Err(failed(&err)),
        }
    }
}
