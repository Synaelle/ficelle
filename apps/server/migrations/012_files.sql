-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Attachment files. Each stored object once: a standard file under its
-- SHA-256 (shared by every space that uploaded it, sealed at rest), an e2ee
-- file under its space and keyed address (as sent). Spaces refer to the
-- objects they use, the bytes live in the storage backend.

CREATE TABLE files (
    object      TEXT PRIMARY KEY,
    -- bytes the account sees: the content for standard files, the sealed
    -- file for e2ee ones
    size        INTEGER NOT NULL,
    -- bytes the backend holds
    stored      INTEGER NOT NULL,
    created_at  INTEGER NOT NULL
) STRICT;

-- a space uses a file. Released references stay for the owner's retention
-- time, then go, then objects nothing refers to
CREATE TABLE file_refs (
    space_id    TEXT NOT NULL REFERENCES sync_spaces (id) ON DELETE CASCADE,
    address     TEXT NOT NULL,
    object      TEXT NOT NULL REFERENCES files (object),
    size        INTEGER NOT NULL,
    added_at    INTEGER NOT NULL,
    released_at INTEGER,
    PRIMARY KEY (space_id, address)
) STRICT;
CREATE INDEX file_refs_object ON file_refs (object);

-- uploads under way, one per space and address
CREATE TABLE file_uploads (
    space_id    TEXT NOT NULL REFERENCES sync_spaces (id) ON DELETE CASCADE,
    address     TEXT NOT NULL,
    size        INTEGER NOT NULL,
    received    INTEGER NOT NULL,
    started_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    PRIMARY KEY (space_id, address)
) STRICT;
