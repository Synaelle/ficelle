-- SPDX-License-Identifier: AGPL-3.0-or-later
-- A QR code on the server's page, for a phone to sign in by scanning it. The
-- page asks for one, the phone's code request names it, the page sees which
-- app answered, the person signs in and approves it. One answer each, five
-- minutes, only the hash of its token is kept.

CREATE TABLE link_offers (
    hash BLOB PRIMARY KEY,
    -- the code request of the app that scanned it, once one did
    link_hash BLOB,
    expires_at INTEGER NOT NULL
) STRICT;
