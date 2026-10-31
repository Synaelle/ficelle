// SPDX-License-Identifier: AGPL-3.0-or-later

//! The server's half of OPAQUE. The server keeps one setup (its OPRF seed
//! and key pair) for its whole life, and per account the record uploaded
//! at registration.

use opaque_ke::{
    CredentialFinalization, CredentialRequest, RegistrationRequest, RegistrationUpload,
    ServerLogin, ServerLoginParameters, ServerRegistration, ServerSetup,
};
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::suite::{CONTEXT, Suite, decode, encode, identifiers, username as normal};
use crate::{Error, Result};

/// The server's long term OPAQUE secrets. Losing them locks every e2ee
/// account out, leaking them lets an attacker holding the records guess
/// passwords offline, at an Argon2id run per guess.
pub struct Setup(ServerSetup<Suite>);

/// A sign-in in progress on the server.
pub struct LoginStarted {
    /// Kept by the server for [`Setup::login_finish`]. Secret.
    pub state: Zeroizing<Vec<u8>>,
    /// Sent to the client.
    pub response: String,
}

impl Setup {
    /// A new setup, made once when the server first starts.
    pub fn generate() -> Self {
        Self(ServerSetup::<Suite>::new(&mut OsRng))
    }

    /// The setup as stored.
    pub fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(self.0.serialize().to_vec())
    }

    /// The setup from storage.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] for bytes that are not a setup.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ServerSetup::<Suite>::deserialize(bytes)
            .map(Self)
            .map_err(|_| Error::Malformed("server setup"))
    }

    /// Answers a registration request for `username`. Holds no state, the
    /// answer only depends on the setup and the username.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] for a request that does not decode.
    pub fn registration_response(&self, request: &str, username: &str) -> Result<String> {
        let request = RegistrationRequest::<Suite>::deserialize(&decode(request, "request")?)
            .map_err(|_| Error::Malformed("request"))?;
        let start =
            ServerRegistration::<Suite>::start(&self.0, request, normal(username).as_bytes())
                .map_err(|_| Error::Malformed("request"))?;
        Ok(encode(&start.message.serialize()))
    }

    /// The record to store from a client's upload.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] for an upload that does not decode.
    pub fn record(upload: &str) -> Result<Vec<u8>> {
        let upload = RegistrationUpload::<Suite>::deserialize(&decode(upload, "upload")?)
            .map_err(|_| Error::Malformed("upload"))?;
        Ok(ServerRegistration::<Suite>::finish(upload)
            .serialize()
            .to_vec())
    }

    /// Starts a sign-in. Without a `record` (no such account, or not an
    /// e2ee one) the answer looks the same and the sign-in fails at the end,
    /// so nobody learns which accounts exist.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] for a request or record that does not decode.
    pub fn login_start(
        &self,
        record: Option<&[u8]>,
        request: &str,
        username: &str,
    ) -> Result<LoginStarted> {
        let record = record
            .map(ServerRegistration::<Suite>::deserialize)
            .transpose()
            .map_err(|_| Error::Malformed("record"))?;
        let request = CredentialRequest::<Suite>::deserialize(&decode(request, "request")?)
            .map_err(|_| Error::Malformed("request"))?;
        let username = normal(username);
        let username = username.as_str();
        let start = ServerLogin::start(
            &mut OsRng,
            &self.0,
            record,
            request,
            username.as_bytes(),
            ServerLoginParameters {
                context: Some(CONTEXT),
                identifiers: identifiers(username),
            },
        )
        .map_err(|_| Error::Malformed("request"))?;
        Ok(LoginStarted {
            state: Zeroizing::new(start.state.serialize().to_vec()),
            response: encode(&start.message.serialize()),
        })
    }

    /// Finishes a sign-in: `Ok` means the client knew the password.
    ///
    /// # Errors
    ///
    /// [`Error::Refused`] for a wrong password or an unknown account,
    /// [`Error::Malformed`] for messages that do not decode.
    pub fn login_finish(state: &[u8], finalization: &str, username: &str) -> Result<()> {
        let username = normal(username);
        let username = username.as_str();
        let login = ServerLogin::<Suite>::deserialize(state)
            .map_err(|_| Error::Malformed("login state"))?;
        let finalization =
            CredentialFinalization::<Suite>::deserialize(&decode(finalization, "finalization")?)
                .map_err(|_| Error::Malformed("finalization"))?;
        login
            .finish(
                finalization,
                ServerLoginParameters {
                    context: Some(CONTEXT),
                    identifiers: identifiers(username),
                },
            )
            .map(|_| ())
            .map_err(|_| Error::Refused)
    }
}
