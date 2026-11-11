// SPDX-License-Identifier: AGPL-3.0-or-later
// The private inbox at `/inbox`: the list by status (spam only when asked
// for), one report with its images, its answer and status, marking spam,
// deleting. Behind Cloudflare Access, see access.ts.

import { reader, type AccessSecrets } from './access'
import {
  listPage,
  messagePage,
  reportPage,
  type Counts,
  type Full,
  type ImageRow,
  type Summary,
} from './pages'
import { readCapped } from './http'
import { imageKey, remove, STATUSES, type Status } from './store'

const PAGE_SIZE = 50
const MAX_ANSWER = 2_000
const ID = /^[0-9a-f]{32}$/

const HEADERS = {
  'content-type': 'text/html; charset=utf-8',
  'cache-control': 'no-store',
  'content-security-policy':
    "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
  'referrer-policy': 'no-referrer',
  'x-content-type-options': 'nosniff',
}

function html(body: string, status = 200): Response {
  return new Response(body, { status, headers: HEADERS })
}

function redirect(to: string): Response {
  return new Response(null, { status: 303, headers: { location: to, 'cache-control': 'no-store' } })
}

async function list(db: D1Database, who: string, url: URL): Promise<Response> {
  const asked = url.searchParams.get('status')
  const status = STATUSES.find((s) => s === asked)
  const args: (string | number)[] = []
  let where = status ? `status = ?${args.push(status)}` : "status != 'spam'"
  const cursor = url.searchParams.get('before')?.match(/^(\d+):([0-9a-f]{32})$/)
  if (cursor) {
    const at = Number(cursor[1])
    where += ` AND (created_at < ?${args.push(at)} OR (created_at = ?${args.length} AND id < ?${args.push(cursor[2] ?? '')}))`
  }
  const [rows, counted] = await db.batch([
    db
      .prepare(
        `SELECT id, created_at, status, category, about, substr(message, 1, 200) AS excerpt,
           version, platform, contact IS NOT NULL AS has_contact, answer != '' AS answered,
           (SELECT count(*) FROM images WHERE report = reports.id) AS images
         FROM reports WHERE ${where} ORDER BY created_at DESC, id DESC LIMIT ${PAGE_SIZE + 1}`,
      )
      .bind(...args),
    db.prepare('SELECT status, count(*) AS n FROM reports GROUP BY status'),
  ])
  const reports = (rows?.results ?? []) as Summary[]
  const counts: Counts = { new: 0, open: 0, done: 0, spam: 0 }
  for (const row of (counted?.results ?? []) as { status: Status; n: number }[]) {
    counts[row.status] = row.n
  }
  const more = reports.length > PAGE_SIZE
  reports.length = Math.min(reports.length, PAGE_SIZE)
  const last = reports.at(-1)
  const next = more && last ? `${last.created_at}:${last.id}` : undefined
  return html(listPage(who, status, counts, reports, next))
}

async function show(db: D1Database, who: string, id: string, saved: boolean): Promise<Response> {
  const [found, stored] = await db.batch([
    db
      .prepare(
        `SELECT id, created_at, status, category, about, message, contact, version, platform,
           logs, install, server, score, flags, answer, receipt IS NOT NULL AS answerable,
           (SELECT count(*) FROM reports o WHERE o.install = reports.install) AS from_install
         FROM reports WHERE id = ?1`,
      )
      .bind(id),
    db
      .prepare(
        'SELECT position, kind, width, height, size FROM images WHERE report = ?1 ORDER BY position',
      )
      .bind(id),
  ])
  const report = found?.results[0] as Full | undefined
  if (!report) return html(messagePage(who, 'No such report', 'It may have been deleted.'), 404)
  return html(reportPage(who, report, (stored?.results ?? []) as ImageRow[], saved))
}

async function image(images: R2Bucket, id: string, position: string): Promise<Response> {
  const object = /^[0-2]$/.test(position) ? await images.get(imageKey(id, Number(position))) : null
  if (!object) return new Response('no such image', { status: 404 })
  return new Response(object.body, {
    headers: {
      'content-type': object.httpMetadata?.contentType ?? 'application/octet-stream',
      'cache-control': 'private, max-age=86400',
      'content-security-policy': "default-src 'none'; sandbox",
      'x-content-type-options': 'nosniff',
    },
  })
}

async function change(
  request: Request,
  env: Env,
  who: string,
  url: URL,
  id: string,
): Promise<Response> {
  // a form from this inbox only: another site cannot post here with the reader's sign-in
  if (request.headers.get('origin') !== url.origin) {
    return html(messagePage(who, 'Not sent from the inbox', 'Open the report and try again.'), 403)
  }
  const body = await readCapped(request, 64 * 1024)
  if (!body) return html(messagePage(who, 'Too long', 'The answer is too long.'), 413)
  const form = new URLSearchParams(new TextDecoder().decode(body))
  const action = form.get('action')
  let changed: D1Result | undefined
  if (action === 'save') {
    const status = form.get('status')
    const answer = form.get('answer')
    if (status !== 'new' && status !== 'open' && status !== 'done') {
      return html(messagePage(who, 'Unknown status', 'Pick new, open or done.'), 400)
    }
    if (answer !== null && [...answer.trim()].length > MAX_ANSWER) {
      return html(
        messagePage(who, 'Too long', `An answer is ${MAX_ANSWER} characters at most.`),
        400,
      )
    }
    changed = await env.DB.prepare(
      "UPDATE reports SET status = ?2, answer = coalesce(?3, answer) WHERE id = ?1 AND status != 'spam'",
    )
      .bind(id, status, answer?.trim() ?? null)
      .run()
  } else if (action === 'spam' || action === 'notSpam') {
    changed = await env.DB.prepare('UPDATE reports SET status = ?2 WHERE id = ?1')
      .bind(id, action === 'spam' ? 'spam' : 'new')
      .run()
  } else if (action === 'delete') {
    await remove(env.DB, env.IMAGES, [id])
    console.log(`feedback report ${id} deleted by ${who}`)
    return redirect('/inbox')
  } else {
    return html(messagePage(who, 'Unknown action', 'Nothing was changed.'), 400)
  }
  if (changed.meta.changes === 0) {
    return html(messagePage(who, 'Nothing changed', 'The report is gone, or is spam.'), 404)
  }
  return redirect(`/inbox/${id}?saved=1`)
}

export async function inbox(
  request: Request,
  env: Env & AccessSecrets,
  url: URL,
): Promise<Response> {
  const who = await reader(request, env, Date.now())
  if (!who) {
    return new Response('the inbox is behind Cloudflare Access', {
      status: 403,
      headers: { 'content-type': 'text/plain; charset=utf-8', 'cache-control': 'no-store' },
    })
  }
  const parts = url.pathname.split('/').filter((p) => p !== '')
  const [, id, sub, position] = parts
  if (parts.length === 1 && request.method === 'GET') return list(env.DB, who, url)
  if (!id || !ID.test(id))
    return html(messagePage(who, 'No such page', 'Go back to the inbox.'), 404)
  if (parts.length === 2 && request.method === 'GET') {
    return show(env.DB, who, id, url.searchParams.has('saved'))
  }
  if (parts.length === 2 && request.method === 'POST') return change(request, env, who, url, id)
  if (parts.length === 4 && sub === 'images' && position && request.method === 'GET') {
    return image(env.IMAGES, id, position)
  }
  return html(messagePage(who, 'No such page', 'Go back to the inbox.'), 404)
}
