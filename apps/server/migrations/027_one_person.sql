-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Servers are for one person: what only served several goes. Invites,
-- groups, single sign-on links, the members and keys of shared spaces,
-- the inbox's readers, and the settings only they read. Accounts, devices,
-- spaces with their changes and the files stay as they are: the notes of a
-- shared space are kept, its owner's devices hold them as their own.

DROP TABLE invites;
DROP TABLE sso_identities;
DROP TABLE sync_grants;
DROP TABLE sync_left;
DROP TABLE sync_members;
DROP TABLE feedback_readers;
DELETE FROM policies WHERE scope IN ('group', 'user');
DROP TABLE group_members;
-- its trigger on the policies goes with it
DROP TABLE groups;
DELETE FROM settings WHERE key IN ('registration', 'account_kinds', 'server_use');
