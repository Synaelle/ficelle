// SPDX-License-Identifier: AGPL-3.0-or-later
// Against floods, as the server has them: requests a minute from one
// address for challenges and answers, reports an hour and a day per
// server, per address and per install, and a total a day. Counted in D1
// from the `asks` and `sends` rows.

export const MINUTE = 60_000
export const HOUR = 60 * MINUTE
export const DAY = 24 * HOUR

/** Challenge and answers requests a minute from one address. */
export const ASKS_PER_MINUTE = 10

/** Reports an hour from one server's people, or one device of an app on no server. */
export const PER_HOUR = 20
/** Reports a day, counted the same way. */
export const PER_DAY = 100
/** Reports a day for the whole Worker. */
export const INSTANCE_PER_DAY = 500

/** Reports from one address whatever server it names: a name in a report proves nothing. */
export const SAFETY_PER_HOUR = 60
export const SAFETY_PER_DAY = 300

function secondsUntil(now: number, until: number): number {
  return Math.floor(Math.max(until - now, 0) / 1000) + 1
}

/**
 * Takes one request from `address`'s allowance. Returns the seconds to wait
 * when it is used up.
 */
export async function ask(
  db: D1Database,
  address: string,
  now: number,
): Promise<number | undefined> {
  const row = await db
    .prepare('SELECT count(*) AS n, min(at) AS oldest FROM asks WHERE address = ?1 AND at > ?2')
    .bind(address, now - MINUTE)
    .first<{ n: number; oldest: number | null }>()
  if (row && row.n >= ASKS_PER_MINUTE) return secondsUntil(now, (row.oldest ?? now) + MINUTE)
  await db.prepare('INSERT INTO asks (address, at) VALUES (?1, ?2)').bind(address, now).run()
  return undefined
}

/** Reports taken in from `address` in the last hour. */
export async function recentFrom(db: D1Database, address: string, now: number): Promise<number> {
  const row = await db
    .prepare('SELECT count(*) AS n FROM sends WHERE address = ?1 AND at > ?2')
    .bind(address, now - HOUR)
    .first<{ n: number }>()
  return row?.n ?? 0
}

export interface Sender {
  /** The keyed hash of the address. */
  address: string
  install: string | null
  /** The server the app says it is signed in to. */
  server: string | null
}

interface Window {
  column: 'address' | 'install' | 'server'
  key: string
  window: number
  limit: number
}

/**
 * The seconds before `sender` may send again, `undefined` when it may now.
 * The caps count per server; an app on no server counts per address and
 * install, at half the caps without an install id. The total and the per
 * address safety hold for everyone.
 */
export async function checkSender(
  db: D1Database,
  now: number,
  sender: Sender,
): Promise<number | undefined> {
  const windows: Window[] = []
  const pair = (column: Window['column'], key: string, hour: number, day: number) =>
    windows.push(
      { column, key, window: HOUR, limit: hour },
      { column, key, window: DAY, limit: day },
    )
  if (sender.server !== null) {
    pair('address', sender.address, SAFETY_PER_HOUR, SAFETY_PER_DAY)
    pair('server', sender.server, PER_HOUR, PER_DAY)
  } else if (sender.install !== null) {
    pair('address', sender.address, PER_HOUR, PER_DAY)
    pair('install', sender.install, PER_HOUR, PER_DAY)
  } else {
    // without an install id, every report of the address is a stranger's
    pair('address', sender.address, PER_HOUR / 2, PER_DAY / 2)
  }
  const dayStart = now - (now % DAY)
  const [today, ...counts] = await db.batch<{ n: number; oldest: number | null }>([
    db.prepare('SELECT count(*) AS n, NULL AS oldest FROM sends WHERE at >= ?1').bind(dayStart),
    ...windows.map((w) =>
      db
        .prepare(
          `SELECT count(*) AS n, min(at) AS oldest FROM sends WHERE ${w.column} = ?1 AND at > ?2`,
        )
        .bind(w.key, now - w.window),
    ),
  ])
  if ((today?.results[0]?.n ?? 0) >= INSTANCE_PER_DAY) return secondsUntil(now, dayStart + DAY)
  for (const [i, w] of windows.entries()) {
    const row = counts[i]?.results[0]
    if (row && row.n >= w.limit) return secondsUntil(now, (row.oldest ?? now) + w.window)
  }
  return undefined
}
