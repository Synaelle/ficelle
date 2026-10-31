-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Instance facts that never change after the first start. Accounts, devices
-- and the rest arrive in the following migrations.

CREATE TABLE instance (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

-- 16 random bytes in hex, identifies this server to its clients
INSERT INTO instance (key, value) VALUES ('id', lower(hex(randomblob(16))));
INSERT INTO instance (key, value) VALUES ('created_at', strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));
