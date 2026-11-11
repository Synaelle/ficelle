-- The Worker's own random key, made on first use: it seals challenges and
-- hashes addresses.
CREATE TABLE meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- status is new, open, done or spam. receipt is the SHA-256 hex of a
-- secret only the sender's app keeps, to read the answer back.
CREATE TABLE reports (
  id TEXT PRIMARY KEY,
  created_at INTEGER NOT NULL,
  status TEXT NOT NULL,
  category TEXT NOT NULL DEFAULT '',
  about TEXT,
  message TEXT NOT NULL,
  contact TEXT,
  version TEXT NOT NULL,
  platform TEXT NOT NULL,
  logs TEXT,
  install TEXT,
  server TEXT,
  score INTEGER NOT NULL,
  flags TEXT NOT NULL,
  answer TEXT NOT NULL DEFAULT '',
  receipt TEXT
);
CREATE INDEX reports_by_status ON reports (status, created_at DESC, id DESC);
CREATE INDEX reports_by_time ON reports (created_at DESC, id DESC);
CREATE INDEX reports_by_receipt ON reports (receipt) WHERE receipt IS NOT NULL;

-- The bytes are in R2, under <report>/<position>.
CREATE TABLE images (
  report TEXT NOT NULL REFERENCES reports (id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  kind TEXT NOT NULL,
  width INTEGER NOT NULL,
  height INTEGER NOT NULL,
  size INTEGER NOT NULL,
  PRIMARY KEY (report, position)
);

-- One row per report taken in, kept a day, for the limits and the
-- repeated message check. address is a keyed hash.
CREATE TABLE sends (
  at INTEGER NOT NULL,
  address TEXT NOT NULL,
  install TEXT,
  message_hash TEXT NOT NULL,
  server TEXT
);
CREATE INDEX sends_by_address ON sends (address, at);
CREATE INDEX sends_by_install ON sends (install, at) WHERE install IS NOT NULL;
CREATE INDEX sends_by_server ON sends (server, at) WHERE server IS NOT NULL;
CREATE INDEX sends_by_hash ON sends (message_hash, at);
CREATE INDEX sends_by_time ON sends (at);

-- Challenges answered, until they expire: each is taken once.
CREATE TABLE used_challenges (
  id TEXT PRIMARY KEY,
  expires_at INTEGER NOT NULL
);

-- Challenge and answers requests, kept a minute, for the per address limit.
CREATE TABLE asks (
  address TEXT NOT NULL,
  at INTEGER NOT NULL
);
CREATE INDEX asks_by_address ON asks (address, at);
