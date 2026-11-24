-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The server sends no email: addresses, codes by email and the links they
-- sent go. An account whose only second step was codes by email signs in
-- with its password, as one with no second step yet: an admin among them
-- adds a passkey or an authenticator app before acting as admin, unless
-- the server lets admins go without. Recovery codes only stand in for a
-- second step, so theirs go too.

UPDATE users SET factor_required = 1
WHERE role = 'admin'
  AND totp_secret IS NULL
  AND NOT EXISTS (SELECT 1 FROM passkeys WHERE passkeys.user_id = users.id)
  AND coalesce((SELECT value FROM settings WHERE key = 'admin_second_step'), '') != 'optional';
DELETE FROM recovery_codes
WHERE user_id IN (
    SELECT id FROM users
    WHERE totp_secret IS NULL
      AND NOT EXISTS (SELECT 1 FROM passkeys WHERE passkeys.user_id = users.id)
);
DELETE FROM challenges WHERE kind IN ('email_verify', 'password_reset');
DROP TABLE email_codes;
ALTER TABLE users DROP COLUMN email_codes;
ALTER TABLE users DROP COLUMN email;
ALTER TABLE users DROP COLUMN email_verified_at;
