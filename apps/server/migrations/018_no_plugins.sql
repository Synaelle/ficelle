-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The apps no longer run plugins: the ones an admin uploaded go. The rule
-- in settings keeps its themes part, the rest is no longer read.

DELETE FROM extensions WHERE kind = 'plugin';
