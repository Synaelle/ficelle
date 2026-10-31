-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Feedback reports, in their own file: nothing here refers to accounts or
-- notes, and nothing there to this. Addresses are never stored, only a
-- keyed hash of them (`address`), enough to count and block.

-- the server's own random values ('secret': challenges and address hashes)
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
) STRICT;

CREATE TABLE reports (
    id         TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    status     TEXT NOT NULL CHECK (status IN ('new', 'open', 'done', 'spam')),
    category   TEXT NOT NULL,
    message    TEXT NOT NULL,
    email      TEXT,
    version    TEXT NOT NULL,
    platform   TEXT NOT NULL,
    logs       TEXT,
    install    TEXT,
    address    TEXT NOT NULL,
    -- the spam score and what made it, comma separated
    score      INTEGER NOT NULL,
    flags      TEXT NOT NULL,
    -- the moderator's words, for example how it was fixed
    note       TEXT NOT NULL DEFAULT '',
    -- kept past the instance's keeping time
    kept       INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX reports_by_time ON reports (created_at);

CREATE TABLE images (
    report   TEXT NOT NULL REFERENCES reports (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    kind     TEXT NOT NULL CHECK (kind IN ('png', 'jpeg', 'webp')),
    width    INTEGER NOT NULL,
    height   INTEGER NOT NULL,
    bytes    BLOB NOT NULL,
    PRIMARY KEY (report, position)
) STRICT;

-- every report taken in, kept or not (spam, blocked), for the limits and
-- for repeated text, kept a day
CREATE TABLE sends (
    at           INTEGER NOT NULL,
    address      TEXT NOT NULL,
    install      TEXT,
    message_hash TEXT NOT NULL
) STRICT;

CREATE INDEX sends_by_time ON sends (at);
CREATE INDEX sends_by_address ON sends (address, at);
CREATE INDEX sends_by_install ON sends (install, at);
CREATE INDEX sends_by_message ON sends (message_hash, at);

-- answered challenges, until they expire, so each is used once
CREATE TABLE used_challenges (
    id         BLOB PRIMARY KEY,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE TABLE blocks (
    kind       TEXT NOT NULL CHECK (kind IN ('install', 'address', 'email', 'word')),
    value      TEXT NOT NULL,
    note       TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    -- addresses change hands, their blocks end, NULL never
    expires_at INTEGER,
    PRIMARY KEY (kind, value)
) STRICT;
