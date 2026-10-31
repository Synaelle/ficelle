-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Groups of people. Sharing (phase 7) and quotas (phase 8) build on them.

CREATE TABLE groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE group_members (
    group_id TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (group_id, user_id)
) STRICT;
CREATE INDEX group_members_user ON group_members (user_id);
