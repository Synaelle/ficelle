-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The server account this vault syncs with, at most one. The tokens and the
-- e2ee account key sit here, inside the encrypted database, never in plain
-- files or the system key store of another app.

CREATE TABLE sync_link (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    server              TEXT NOT NULL,
    account_id          TEXT NOT NULL,
    username            TEXT NOT NULL,
    -- standard or e2ee
    kind                TEXT NOT NULL,
    -- e2ee: the account master key, every document key derives from it
    master_key          BLOB,
    -- this device's id on the server, to skip its own changes when pulling
    device_id           TEXT NOT NULL,
    access_token        TEXT,
    access_expires_at   INTEGER,
    refresh_token       TEXT,
    refresh_expires_at  INTEGER,
    -- number and name of the last change taken from the server: the server
    -- names the change under that number on each pull, another name means
    -- its log is not the one followed here
    cursor              INTEGER NOT NULL DEFAULT 0,
    cursor_id           TEXT,
    linked_at           INTEGER NOT NULL,
    last_sync_at        INTEGER,
    last_error          TEXT
);

-- changes from the server that could not be read (a key that does not open
-- them, damaged bytes). Kept rather than dropped, and they never block the
-- changes after them
CREATE TABLE sync_rejected (
    seq         INTEGER PRIMARY KEY,
    doc_id      TEXT NOT NULL,
    reason      TEXT NOT NULL,
    bytes       BLOB NOT NULL,
    at          INTEGER NOT NULL
);

-- changes that arrived before a change they build on. Loro holds them until
-- it comes, this keeps them across restarts. Retried after each change to
-- their document, dropped once applied
CREATE TABLE sync_waiting (
    seq         INTEGER PRIMARY KEY,
    doc_id      TEXT NOT NULL,
    bytes       BLOB NOT NULL
);
CREATE INDEX sync_waiting_doc ON sync_waiting (doc_id);
