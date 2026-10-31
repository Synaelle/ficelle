-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Files policies set in the admin page, for the instance, a group or a
-- person: json of the keys set at that level, the others inherited.

CREATE TABLE policies (
    scope       TEXT NOT NULL CHECK (scope IN ('instance', 'group', 'user')),
    -- '' for the instance, else the group or account id
    target      TEXT NOT NULL,
    settings    TEXT NOT NULL,
    updated_at  INTEGER NOT NULL,
    PRIMARY KEY (scope, target)
) STRICT;

CREATE TRIGGER policies_group_gone AFTER DELETE ON groups
BEGIN
    DELETE FROM policies WHERE scope = 'group' AND target = OLD.id;
END;

CREATE TRIGGER policies_user_gone AFTER DELETE ON users
BEGIN
    DELETE FROM policies WHERE scope = 'user' AND target = OLD.id;
END;
