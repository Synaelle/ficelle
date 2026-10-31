// SPDX-License-Identifier: AGPL-3.0-or-later

//! Bringing notes in from other apps. The formats are read by the page
//! (`lib/import`), which has the web view's HTML and XML parsers; the core
//! reads the exports themselves ([`source`]) and writes what the page found
//! ([`write`]).

pub mod source;
