-- SPDX-License-Identifier: AGPL-3.0-or-later
-- What nothing reads any more: the themes the admin once offered, the
-- marks and keys of shared spaces, and policies for groups and members.
-- Policies are made again to narrow their scope, SQLite cannot change a
-- check in place. The trigger that removed a member's policy goes first,
-- it sits on the accounts and would point at nothing.

DROP TABLE extensions;
ALTER TABLE sync_spaces DROP COLUMN shared;
ALTER TABLE sync_spaces DROP COLUMN epoch;
ALTER TABLE users DROP COLUMN share_public;
ALTER TABLE users DROP COLUMN share_private;

DROP TRIGGER policies_user_gone;
CREATE TABLE policies_new (
    scope       TEXT NOT NULL CHECK (scope = 'instance'),
    -- always ''
    target      TEXT NOT NULL,
    settings    TEXT NOT NULL,
    updated_at  INTEGER NOT NULL,
    PRIMARY KEY (scope, target)
) STRICT;
INSERT INTO policies_new (scope, target, settings, updated_at)
    SELECT scope, target, settings, updated_at FROM policies WHERE scope = 'instance';
DROP TABLE policies;
ALTER TABLE policies_new RENAME TO policies;
