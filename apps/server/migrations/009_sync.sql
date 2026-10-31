-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Sync: each account's space is a log of document changes (Loro updates)
-- in the order the server took them. Devices read the log from where they
-- left off. Standard accounts' changes are encrypted here with a key per
-- space, itself wrapped by the server master key. E2ee accounts' changes
-- arrive encrypted by the devices and are kept as sent.

CREATE TABLE sync_spaces (
    id          TEXT PRIMARY KEY,
    owner_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- standard: the space key wrapped by the master key; NULL for e2ee
    data_key    BLOB,
    created_at  INTEGER NOT NULL
) STRICT;
CREATE INDEX sync_spaces_owner ON sync_spaces (owner_id);

-- seq is global and only grows, a space reads its own rows in seq order
CREATE TABLE sync_changes (
    seq         INTEGER PRIMARY KEY,
    space_id    TEXT NOT NULL REFERENCES sync_spaces (id) ON DELETE CASCADE,
    doc_id      TEXT NOT NULL,
    -- the device that sent it, NULL once that device is gone
    device_id   TEXT REFERENCES devices (id) ON DELETE SET NULL,
    -- the device's own id for the change: a resend is stored once
    change_id   TEXT NOT NULL,
    bytes       BLOB NOT NULL,
    size        INTEGER NOT NULL,
    created_at  INTEGER NOT NULL,
    UNIQUE (space_id, change_id)
) STRICT;
CREATE INDEX sync_changes_space ON sync_changes (space_id, seq);

-- e2ee: each document's key, wrapped by the account's master key on a device
CREATE TABLE sync_doc_keys (
    space_id    TEXT NOT NULL REFERENCES sync_spaces (id) ON DELETE CASCADE,
    doc_id      TEXT NOT NULL,
    wrapped     TEXT NOT NULL,
    PRIMARY KEY (space_id, doc_id)
) STRICT;
