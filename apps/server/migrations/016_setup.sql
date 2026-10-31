-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Setup is locked for good once the first admin exists. Instances made
-- before the lock existed are already past it. The old setup code goes.
INSERT INTO instance (key, value)
SELECT 'setup_done', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
WHERE EXISTS (SELECT 1 FROM users);
DELETE FROM instance WHERE key = 'setup_code';
