// SPDX-License-Identifier: AGPL-3.0-or-later
// The three calls the apps make, as the server answers them:
// `POST /api/v1/feedback/challenge`, `POST /api/v1/feedback` and
// `POST /api/v1/feedback/answers`. No account is read, a report is
// anonymous.

import { bitsFor, makeChallenge, REFUSED, takeChallenge } from './challenge'
import { apiError, invalid, json, noContent, rateLimited, readCapped } from './http'
import { addressKey } from './keys'
import { ask, checkSender, recentFrom } from './limits'
import { MAX_REQUEST, sentProof, validate } from './report'
import { answers as answersOf, MAX_ASKED, take } from './store'

/** Most of an answers request: 50 secrets of 128 characters. */
const MAX_ASKED_BODY = 16 * 1024

/** The address the request came from, as Cloudflare saw it. */
function clientIp(request: Request): string {
  return request.headers.get('cf-connecting-ip') ?? ''
}

function powBits(env: Env): number {
  const bits = Number(env.POW_BITS)
  if (!Number.isInteger(bits) || bits < 8 || bits > 28) {
    throw new Error('POW_BITS must be between 8 and 28')
  }
  return bits
}

async function readJson(request: Request, max: number): Promise<unknown | Response> {
  const body = await readCapped(request, max)
  if (!body) return apiError(413, 'invalid', 'this report is too large')
  try {
    return JSON.parse(new TextDecoder().decode(body)) as unknown
  } catch (err) {
    return invalid(`not a report: ${(err as Error).message}`)
  }
}

export async function challenge(request: Request, env: Env): Promise<Response> {
  const now = Date.now()
  const address = await addressKey(env.DB, clientIp(request))
  const wait = await ask(env.DB, address, now)
  if (wait !== undefined) return rateLimited(wait)
  const recent = await recentFrom(env.DB, address, now)
  return json(await makeChallenge(env.DB, now, bitsFor(powBits(env), recent)))
}

/** Answers 204 whether the report was kept or filed as spam, so a sender learns nothing of the filters. */
export async function send(request: Request, env: Env): Promise<Response> {
  const body = await readJson(request, MAX_REQUEST)
  if (body instanceof Response) return body
  const proof = sentProof(body)
  if (typeof proof === 'string') return invalid(proof)
  const now = Date.now()
  // the work is checked before anything else counts, a limit is never
  // found out for free, and no image is decoded for a sender who did none
  const refused = await takeChallenge(env.DB, now, proof.challenge, proof.nonce)
  if (refused) return apiError(400, 'challengeInvalid', REFUSED[refused])
  const incoming = validate(body as Record<string, unknown>)
  if (typeof incoming === 'string') return invalid(incoming)
  const address = await addressKey(env.DB, clientIp(request))
  const wait = await checkSender(env.DB, now, {
    address,
    install: incoming.install,
    server: incoming.server,
  })
  if (wait !== undefined) return rateLimited(wait)
  const { id, spam } = await take(env.DB, env.IMAGES, now, address, incoming)
  console.log(spam ? `feedback report ${id} filed as spam` : `feedback report ${id}`)
  return noContent()
}

/** How a sender's own reports stand, by the secrets only their app keeps. */
export async function answers(request: Request, env: Env): Promise<Response> {
  const now = Date.now()
  const address = await addressKey(env.DB, clientIp(request))
  const wait = await ask(env.DB, address, now)
  if (wait !== undefined) return rateLimited(wait)
  const body = await readJson(request, MAX_ASKED_BODY)
  if (body instanceof Response) return body
  const secrets = (body as { secrets?: unknown } | null)?.secrets
  if (
    !Array.isArray(secrets) ||
    secrets.length > MAX_ASKED ||
    secrets.some((s) => typeof s !== 'string' || s === '' || s.length > 128)
  ) {
    return invalid('too many secrets, or not secrets')
  }
  return json({ reports: await answersOf(env.DB, secrets as string[]) })
}
