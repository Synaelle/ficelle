-- SPDX-License-Identifier: AGPL-3.0-or-later
-- How each e2ee account's OPAQUE record stretches the password: 1 for
-- records made before 0.3.0 (Argon2id 19 MiB, 2 passes), 2 since (64 MiB,
-- 3 passes). The apps register an old record again at their next sign-in.

ALTER TABLE users ADD COLUMN opaque_ksf INTEGER NOT NULL DEFAULT 1;
