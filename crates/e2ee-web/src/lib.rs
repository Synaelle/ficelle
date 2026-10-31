// SPDX-License-Identifier: AGPL-3.0-or-later

//! The e2ee account client as WebAssembly, for the admin page and the
//! invite page. Answers are json text, the page parses them. The export key
//! and the master key never cross into JavaScript: the page only gets what
//! it sends to the server, and the recovery code to show once.

use app_e2ee::{
    AccountKeys, KeyBundle, Ksf, login_finish, login_start, new_account, registration_finish,
    registration_start,
};
use serde_json::json;
use wasm_bindgen::prelude::*;

fn fail(err: &app_e2ee::Error) -> JsError {
    JsError::new(&err.to_string())
}

/// First step of signing in: `{"state", "request"}`.
///
/// # Errors
///
/// Does not fail in practice.
#[wasm_bindgen(js_name = signInStart)]
pub fn sign_in_start(password: &str) -> Result<String, JsError> {
    let start = login_start(password).map_err(|e| fail(&e))?;
    Ok(json!({ "state": *start.state, "request": start.request }).to_string())
}

/// Last step of signing in: `{"finalization"}`. `ksf` is the number the
/// server said the record stretches the password with.
///
/// # Errors
///
/// A wrong password, a server answer that does not check out, or a
/// stretching this build does not know.
#[wasm_bindgen(js_name = signInFinish)]
pub fn sign_in_finish(
    state: &str,
    password: &str,
    response: &str,
    username: &str,
    ksf: u8,
) -> Result<String, JsError> {
    let ksf = Ksf::from_number(ksf).ok_or_else(|| JsError::new("unknown password stretching"))?;
    let finish = login_finish(state, password, response, username, ksf).map_err(|e| fail(&e))?;
    Ok(json!({ "finalization": finish.finalization }).to_string())
}

/// First step of signing up: `{"state", "request"}`.
///
/// # Errors
///
/// Does not fail in practice.
#[wasm_bindgen(js_name = signUpStart)]
pub fn sign_up_start(password: &str) -> Result<String, JsError> {
    let start = registration_start(password).map_err(|e| fail(&e))?;
    Ok(json!({ "state": *start.state, "request": start.request }).to_string())
}

/// Last step of signing up, making the account keys:
/// `{"upload", "ksf", "keys", "recoveryVerifier", "recoveryCode"}`.
///
/// # Errors
///
/// A server answer that does not check out.
#[wasm_bindgen(js_name = signUpFinish)]
pub fn sign_up_finish(
    state: &str,
    password: &str,
    response: &str,
    username: &str,
) -> Result<String, JsError> {
    let finish = registration_finish(state, password, response, username, Ksf::CURRENT)
        .map_err(|e| fail(&e))?;
    let account = new_account(&finish.export_key);
    Ok(json!({
        "upload": finish.upload,
        "ksf": Ksf::CURRENT.number(),
        "keys": account.bundle,
        "recoveryVerifier": account.recovery_verifier,
        "recoveryCode": *account.recovery_code,
    })
    .to_string())
}

/// Proof of a recovery code, for `/auth/recovery/start`.
///
/// # Errors
///
/// Text that is not a recovery code.
#[wasm_bindgen(js_name = recoveryVerifier)]
pub fn recovery_verifier(code: &str) -> Result<String, JsError> {
    AccountKeys::recovery_verifier(code).map_err(|e| fail(&e))
}

/// Last step of a recovery: the new password's upload and the keys with
/// the password wrapping redone, `{"upload", "ksf", "keys"}`. The recovery code
/// opens the master key here again, so the key never reaches JavaScript.
///
/// # Errors
///
/// A code that does not open these keys, or a server answer that does not
/// check out.
#[wasm_bindgen(js_name = recoverFinish)]
pub fn recover_finish(
    code: &str,
    keys: &str,
    state: &str,
    password: &str,
    response: &str,
    username: &str,
) -> Result<String, JsError> {
    let bundle: KeyBundle =
        serde_json::from_str(keys).map_err(|_| JsError::new("malformed keys"))?;
    let master = AccountKeys::open_with_recovery(&bundle, code).map_err(|e| fail(&e))?;
    let finish = registration_finish(state, password, response, username, Ksf::CURRENT)
        .map_err(|e| fail(&e))?;
    let bundle = AccountKeys::rewrap_password(&bundle, &master, &finish.export_key);
    Ok(
        json!({ "upload": finish.upload, "ksf": Ksf::CURRENT.number(), "keys": bundle })
            .to_string(),
    )
}
