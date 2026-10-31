// SPDX-License-Identifier: AGPL-3.0-or-later

//! The device's user verification, where the platform has one wired in:
//! Windows Hello, and the Android Keystore with fingerprint or face. Touch ID
//! and Face ID on Apple plug in here too (untested until CI builds them).

use app_core::keystore::{Authenticator, GuardedKeyStore, KeyStore};

/// The verification and where its sealed secrets are kept.
type Platform = (&'static dyn Authenticator, &'static dyn KeyStore);

#[cfg(windows)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "same signature on every platform, others return None"
)]
fn platform() -> Option<Platform> {
    use app_core::keystore::OsKeyStore;
    Some((&crate::hello::WindowsHello, &OsKeyStore))
}

#[cfg(target_os = "android")]
fn platform() -> Option<Platform> {
    use crate::android_keystore::{AndroidKeystore, sealed_files};
    sealed_files().map(|files| {
        (
            &AndroidKeystore as &dyn Authenticator,
            files as &dyn KeyStore,
        )
    })
}

#[cfg(not(any(windows, target_os = "android")))]
fn platform() -> Option<Platform> {
    None
}

/// Where this device keeps a vault's key for the "this device" way: the
/// system's key store on desktop (Credential Manager on Windows), the app's
/// private storage on Android (encrypted by the phone with its screen lock).
/// `None` when neither works here.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn device_store() -> Option<&'static dyn KeyStore> {
    use app_core::keystore::OsKeyStore;
    // a store that cannot be read means this device cannot keep a key
    OsKeyStore
        .get("availability-probe")
        .is_ok()
        .then_some(&OsKeyStore as &dyn KeyStore)
}

#[cfg(target_os = "android")]
pub fn device_store() -> Option<&'static dyn KeyStore> {
    crate::android_keystore::sealed_files().map(|files| files as &dyn KeyStore)
}

#[cfg(target_os = "ios")]
pub fn device_store() -> Option<&'static dyn KeyStore> {
    None
}

/// Key store that only releases the database key after verification, or
/// `None` when this platform has no verification wired in.
pub fn guarded() -> Option<GuardedKeyStore<&'static dyn Authenticator, &'static dyn KeyStore>> {
    platform().map(|(auth, inner)| GuardedKeyStore::new(auth, inner))
}
