-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The server a report's app is signed in to, as the app says: the admin's
-- caps count per server, a report from an app on no server counts per install.

ALTER TABLE sends ADD COLUMN server TEXT;

CREATE INDEX sends_by_server ON sends (server, at);
