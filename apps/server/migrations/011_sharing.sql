-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Sharing: a shared space is a space like an account's own, owned by the
-- account that shared it, with members added one by one or as groups. An
-- e2ee space's key is sealed by the owner's devices for each member, the
-- server only passes the sealed keys on.

-- 0 for an account's own space, 1 for a shared one
ALTER TABLE sync_spaces ADD COLUMN shared INTEGER NOT NULL DEFAULT 0;
-- the owner's kind when shared: an e2ee space has only e2ee members
ALTER TABLE sync_spaces ADD COLUMN kind TEXT NOT NULL DEFAULT 'standard' CHECK (kind IN ('standard', 'e2ee'));
-- e2ee: the latest key epoch, a new one starts when someone leaves
ALTER TABLE sync_spaces ADD COLUMN epoch INTEGER NOT NULL DEFAULT 0;

-- one of user_id and group_id is set
CREATE TABLE sync_members (
    space_id    TEXT NOT NULL REFERENCES sync_spaces (id) ON DELETE CASCADE,
    user_id     TEXT REFERENCES users (id) ON DELETE CASCADE,
    group_id    TEXT REFERENCES groups (id) ON DELETE CASCADE,
    role        TEXT NOT NULL CHECK (role IN ('edit', 'read')),
    added_at    INTEGER NOT NULL,
    CHECK ((user_id IS NULL) != (group_id IS NULL))
) STRICT;
CREATE UNIQUE INDEX sync_members_user ON sync_members (space_id, user_id) WHERE user_id IS NOT NULL;
CREATE UNIQUE INDEX sync_members_group ON sync_members (space_id, group_id) WHERE group_id IS NOT NULL;
CREATE INDEX sync_members_of_user ON sync_members (user_id);
CREATE INDEX sync_members_of_group ON sync_members (group_id);

-- someone in a member group who left the space themselves
CREATE TABLE sync_left (
    space_id    TEXT NOT NULL REFERENCES sync_spaces (id) ON DELETE CASCADE,
    user_id     TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (space_id, user_id)
) STRICT;

-- e2ee: a space key sealed for one member, for one epoch
CREATE TABLE sync_grants (
    space_id    TEXT NOT NULL REFERENCES sync_spaces (id) ON DELETE CASCADE,
    user_id     TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    epoch       INTEGER NOT NULL,
    grant_data  TEXT NOT NULL,
    PRIMARY KEY (space_id, user_id, epoch)
) STRICT;
CREATE INDEX sync_grants_user ON sync_grants (user_id);

-- e2ee accounts' key pairs for sharing: the public key for everyone, the
-- private one sealed by the account's master key on a device
ALTER TABLE users ADD COLUMN share_public TEXT;
ALTER TABLE users ADD COLUMN share_private TEXT;
