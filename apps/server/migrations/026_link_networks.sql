-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The network each waiting code and QR code was asked from, so one network
-- cannot take every place there is.

ALTER TABLE app_links ADD COLUMN network TEXT;
ALTER TABLE link_offers ADD COLUMN network TEXT;
