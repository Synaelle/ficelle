// SPDX-License-Identifier: AGPL-3.0-or-later

//! The server as a WebAuthn relying party. Passkeys are bound to a web
//! origin, so they need `server.public_url`: without it they are off.

use std::sync::Arc;

use webauthn_rs::prelude::Url;
use webauthn_rs::{Webauthn, WebauthnBuilder};

use crate::config::Config;

/// The relying party for `public_url`, `None` when it is not set or not
/// usable (an address without a host name, for example).
pub fn relying_party(config: &Config) -> Option<Arc<Webauthn>> {
    let url = &config.server.public_url;
    if url.is_empty() {
        return None;
    }
    let origin = Url::parse(url).ok()?;
    let host = origin.host_str()?.to_owned();
    match WebauthnBuilder::new(&host, &origin)
        .and_then(|b| b.rp_name(app_brand::DISPLAY_NAME).build())
    {
        Ok(webauthn) => Some(Arc::new(webauthn)),
        Err(err) => {
            tracing::warn!(
                "passkeys are off: server.public_url {url:?} does not work for them: {err}"
            );
            None
        }
    }
}
