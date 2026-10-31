// SPDX-License-Identifier: AGPL-3.0-or-later

//! The client's half of OPAQUE. Each exchange is two steps around one
//! server request: `start` gives a message for the server and a state to
//! keep, `finish` takes the server's answer and that state back.

use opaque_ke::{
    ClientLogin, ClientLoginFinishParameters, ClientRegistration,
    ClientRegistrationFinishParameters, CredentialResponse, RegistrationResponse,
};
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::suite::{CONTEXT, Ksf, Suite, decode, encode, identifiers, username as normal};
use crate::{Error, Result};

/// First step of setting a password.
pub struct RegistrationStart {
    /// Kept by the client for [`registration_finish`]. Secret.
    pub state: Zeroizing<String>,
    /// Sent to the server.
    pub request: String,
}

/// Last step of setting a password.
pub struct RegistrationFinish {
    /// Sent to the server, which stores it as the account's record.
    pub upload: String,
    /// Root of the account keys. Never leaves the client.
    pub export_key: Zeroizing<Vec<u8>>,
}

/// First step of signing in.
pub struct LoginStart {
    /// Kept by the client for [`login_finish`]. Secret.
    pub state: Zeroizing<String>,
    /// Sent to the server.
    pub request: String,
}

/// Last step of signing in.
pub struct LoginFinish {
    /// Sent to the server, proves the password.
    pub finalization: String,
    /// Root of the account keys, the same as at registration. Never leaves
    /// the client.
    pub export_key: Zeroizing<Vec<u8>>,
}

/// Starts setting `password`, for a new account or a new password.
///
/// # Errors
///
/// Only when the password cannot be blinded, which does not happen.
pub fn registration_start(password: &str) -> Result<RegistrationStart> {
    let start = ClientRegistration::<Suite>::start(&mut OsRng, password.as_bytes())
        .map_err(|_| Error::Refused)?;
    Ok(RegistrationStart {
        state: Zeroizing::new(encode(&start.state.serialize())),
        request: encode(&start.message.serialize()),
    })
}

/// Finishes setting the password with the server's `response`, the
/// record stretched by `ksf` ([`Ksf::CURRENT`] but in tests).
///
/// # Errors
///
/// [`Error::Malformed`] for messages that do not decode, [`Error::Refused`]
/// for a response that does not check out.
pub fn registration_finish(
    state: &str,
    password: &str,
    response: &str,
    username: &str,
    ksf: Ksf,
) -> Result<RegistrationFinish> {
    let state = Zeroizing::new(decode(state, "registration state")?);
    let client = ClientRegistration::<Suite>::deserialize(&state)
        .map_err(|_| Error::Malformed("registration state"))?;
    let response = RegistrationResponse::<Suite>::deserialize(&decode(response, "response")?)
        .map_err(|_| Error::Malformed("response"))?;
    let finish = client
        .finish(
            &mut OsRng,
            password.as_bytes(),
            response,
            ClientRegistrationFinishParameters::new(
                identifiers(&normal(username)),
                Some(&ksf.argon2()),
            ),
        )
        .map_err(|_| Error::Refused)?;
    Ok(RegistrationFinish {
        upload: encode(&finish.message.serialize()),
        export_key: Zeroizing::new(finish.export_key.to_vec()),
    })
}

/// Starts signing in with `password`.
///
/// # Errors
///
/// Only when the password cannot be blinded, which does not happen.
pub fn login_start(password: &str) -> Result<LoginStart> {
    let start =
        ClientLogin::<Suite>::start(&mut OsRng, password.as_bytes()).map_err(|_| Error::Refused)?;
    Ok(LoginStart {
        state: Zeroizing::new(encode(&start.state.serialize())),
        request: encode(&start.message.serialize()),
    })
}

/// Finishes signing in with the server's `response`, stretching the
/// password as the server said the record was made (`ksf`).
///
/// # Errors
///
/// [`Error::Refused`] for a wrong password, an unknown account, or a server
/// that is not the one holding the record: OPAQUE does not say which.
pub fn login_finish(
    state: &str,
    password: &str,
    response: &str,
    username: &str,
    ksf: Ksf,
) -> Result<LoginFinish> {
    let state = Zeroizing::new(decode(state, "login state")?);
    let client =
        ClientLogin::<Suite>::deserialize(&state).map_err(|_| Error::Malformed("login state"))?;
    let response = CredentialResponse::<Suite>::deserialize(&decode(response, "response")?)
        .map_err(|_| Error::Malformed("response"))?;
    let finish = client
        .finish(
            &mut OsRng,
            password.as_bytes(),
            response,
            ClientLoginFinishParameters::new(
                Some(CONTEXT),
                identifiers(&normal(username)),
                Some(&ksf.argon2()),
            ),
        )
        .map_err(|_| Error::Refused)?;
    Ok(LoginFinish {
        finalization: encode(&finish.message.serialize()),
        export_key: Zeroizing::new(finish.export_key.to_vec()),
    })
}
