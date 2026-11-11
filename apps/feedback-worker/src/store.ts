// SPDX-License-Identifier: AGPL-3.0-or-later
// Reports in D1, their images in R2 under `<report>/<position>`. A report
// is filed as spam when it scores so, never refused for it, and every one
// counts against the limits. Reports are kept 180 days.

import { MIME } from './images'
import { newId, sha256Hex } from './keys'
import { DAY, MINUTE } from './limits'
import { SPAM_SCORE, spamScore, type Incoming } from './report'

export const KEEP_DAYS = 180

/** The most reports asked about at once. */
export const MAX_ASKED = 50

export type Status = 'new' | 'open' | 'done' | 'spam'

export const STATUSES: readonly Status[] = ['new', 'open', 'done', 'spam']

export function imageKey(report: string, position: number): string {
  return `${report}/${position}`
}

/** The same words, whatever their case and spacing. */
async function messageHash(text: string): Promise<string> {
  const normal = text
    .toLowerCase()
    .split(/\p{White_Space}+/u)
    .filter((w) => w !== '')
    .join(' ')
  return (await sha256Hex(normal)).slice(0, 32)
}

/** Takes in a checked report from `address`. Returns its id and whether it went to spam. */
export async function take(
  db: D1Database,
  images: R2Bucket,
  now: number,
  address: string,
  incoming: Incoming,
): Promise<{ id: string; spam: boolean }> {
  // the about line and the words are judged together
  const text = incoming.about === null ? incoming.message : `${incoming.about}\n${incoming.message}`
  const hash = await messageHash(text)
  const repeated = await db
    .prepare('SELECT 1 AS found FROM sends WHERE message_hash = ?1 AND at > ?2 LIMIT 1')
    .bind(hash, now - DAY)
    .first()
  const { score, flags } = spamScore(text.trim(), repeated !== null)
  const spam = score >= SPAM_SCORE
  const id = newId(now)
  const keys = incoming.images.map((_, position) => imageKey(id, position))
  await Promise.all(
    incoming.images.map(({ info, bytes }, position) =>
      images.put(imageKey(id, position), bytes, { httpMetadata: { contentType: MIME[info.kind] } }),
    ),
  )
  try {
    await db.batch([
      db
        .prepare(
          `INSERT INTO reports (id, created_at, status, category, about, message, contact,
             version, platform, logs, install, server, score, flags, receipt)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)`,
        )
        .bind(
          id,
          now,
          spam ? 'spam' : 'new',
          incoming.category,
          incoming.about,
          incoming.message,
          incoming.contact,
          incoming.version,
          incoming.platform,
          incoming.logs,
          incoming.install,
          incoming.server,
          score,
          flags.join(','),
          incoming.receipt,
        ),
      ...incoming.images.map(({ info, bytes }, position) =>
        db
          .prepare(
            `INSERT INTO images (report, position, kind, width, height, size)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)`,
          )
          .bind(id, position, info.kind, info.width, info.height, bytes.length),
      ),
      db
        .prepare(
          'INSERT INTO sends (at, address, install, message_hash, server) VALUES (?1, ?2, ?3, ?4, ?5)',
        )
        .bind(now, address, incoming.install, hash, incoming.server),
    ])
  } catch (err) {
    if (keys.length > 0) await images.delete(keys)
    throw err
  }
  return { id, spam }
}

export type SentState = 'received' | 'open' | 'done'

/**
 * How the reports of these receipts' secrets stand, for their sender. A
 * secret that matches nothing is left out. Spam reads as received: nothing
 * of the filters shows.
 */
export async function answers(
  db: D1Database,
  secrets: string[],
): Promise<{ secret: string; state: SentState; answer: string }[]> {
  if (secrets.length === 0) return []
  const receipts = await Promise.all(secrets.map(sha256Hex))
  const found = await db
    .prepare(
      `SELECT receipt, status, answer FROM reports
       WHERE receipt IN (${receipts.map((_, i) => `?${i + 1}`).join(', ')})`,
    )
    .bind(...receipts)
    .all<{ receipt: string; status: string; answer: string }>()
  const byReceipt = new Map(found.results.map((r) => [r.receipt, r]))
  return secrets.flatMap((secret, i) => {
    const row = byReceipt.get(receipts[i] ?? '')
    if (!row) return []
    const state: SentState =
      row.status === 'open' ? 'open' : row.status === 'done' ? 'done' : 'received'
    return [{ secret, state, answer: row.answer }]
  })
}

/** Deletes reports and their images. */
export async function remove(db: D1Database, images: R2Bucket, ids: string[]): Promise<number> {
  if (ids.length === 0) return 0
  const list = ids.map((_, i) => `?${i + 1}`).join(', ')
  const stored = await db
    .prepare(`SELECT report, position FROM images WHERE report IN (${list})`)
    .bind(...ids)
    .all<{ report: string; position: number }>()
  const keys = stored.results.map((r) => imageKey(r.report, r.position))
  if (keys.length > 0) await images.delete(keys)
  const [, gone] = await db.batch([
    db.prepare(`DELETE FROM images WHERE report IN (${list})`).bind(...ids),
    db.prepare(`DELETE FROM reports WHERE id IN (${list})`).bind(...ids),
  ])
  return gone?.meta.changes ?? 0
}

/** Reports deleted in one go by the daily cleanup, within D1's bound parameters. */
const CLEANUP_BATCH = 90

/**
 * Removes what has had its time: reports past the keeping time, sends after
 * a day, expired challenges and old asks.
 */
export async function cleanup(db: D1Database, images: R2Bucket, now: number): Promise<number> {
  let removed = 0
  // a few rounds a day keep within the queries a run may make
  for (let round = 0; round < 10; round++) {
    const old = await db
      .prepare('SELECT id FROM reports WHERE created_at < ?1 LIMIT ?2')
      .bind(now - KEEP_DAYS * DAY, CLEANUP_BATCH)
      .all<{ id: string }>()
    removed += await remove(
      db,
      images,
      old.results.map((r) => r.id),
    )
    if (old.results.length < CLEANUP_BATCH) break
  }
  const results = await db.batch([
    db.prepare('DELETE FROM sends WHERE at < ?1').bind(now - DAY),
    db.prepare('DELETE FROM used_challenges WHERE expires_at <= ?1').bind(now),
    db.prepare('DELETE FROM asks WHERE at <= ?1').bind(now - MINUTE),
  ])
  return removed + results.reduce((n, r) => n + r.meta.changes, 0)
}
