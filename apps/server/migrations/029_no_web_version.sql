-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The server no longer serves the web version: its sign-in cookies and the
-- setting for who could open it go.

DROP TABLE web_sessions;
DELETE FROM settings WHERE key = 'web_version';
