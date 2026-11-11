-- Devices this vault syncs with directly, on the same network. A device is
-- known by its long-term public key, pinned when it was paired or learned
-- from a device that paired it. A forgotten device stays as a row, so a
-- device that has not heard of it yet cannot bring it back.
CREATE TABLE peer_devices (
    key BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    platform TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT '',
    -- the address it last answered on, tried before discovery has
    address TEXT,
    paired_at INTEGER NOT NULL,
    last_sync_at INTEGER,
    forgotten_at INTEGER
) WITHOUT ROWID;
