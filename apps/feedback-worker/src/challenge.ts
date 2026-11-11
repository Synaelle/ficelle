// SPDX-License-Identifier: AGPL-3.0-or-later
// The proof of work before a report, as the server does it. A challenge is
// stateless: a random id, when it expires and the bits asked, sealed with
// the Worker's key. The app finds a nonce such that SHA-256 of the
// challenge's text then the nonce (8 bytes, little endian) starts with that
// many zero bits. Answered challenges are kept until they expire, so each
// is taken once.

import { concat, hex, mac, sha256 } from './keys'

/** How long a challenge is accepted, in ms. */
export const CHALLENGE_LIFE = 10 * 60 * 1000

/** The work never asks more than this for busy senders, unless the starting point is higher. */
const MAX_BUSY_BITS = 24

const SEAL = 'feedback challenge\0'

const encoder = new TextEncoder()

export interface Challenge {
  challenge: string
  bits: number
  expiresAt: number
}

export type Refused = 'unknown' | 'expired' | 'used' | 'notSolved'

export const REFUSED: Record<Refused, string> = {
  unknown: 'this challenge is not from this server',
  expired: 'the challenge expired, ask for a new one',
  used: 'the challenge was used already, ask for a new one',
  notSolved: 'the nonce does not answer the challenge',
}

export function leadingZeroBits(hash: Uint8Array): number {
  let bits = 0
  for (const byte of hash) {
    if (byte !== 0) return bits + Math.clz32(byte) - 24
    bits += 8
  }
  return bits
}

function nonceBytes(nonce: number): Uint8Array {
  const bytes = new Uint8Array(8)
  new DataView(bytes.buffer).setBigUint64(0, BigInt(nonce), true)
  return bytes
}

/** Whether `nonce` answers `challenge` at `bits`. */
export async function meets(challenge: string, nonce: number, bits: number): Promise<boolean> {
  const hash = await sha256(concat(encoder.encode(challenge), nonceBytes(nonce)))
  return leadingZeroBits(hash) >= bits
}

function toBase64Url(bytes: Uint8Array): string {
  return bytes.toBase64({ alphabet: 'base64url', omitPadding: true })
}

function fromBase64Url(text: string): Uint8Array | undefined {
  if (!/^[A-Za-z0-9_-]*$/.test(text)) return undefined
  try {
    return Uint8Array.fromBase64(text, { alphabet: 'base64url' })
  } catch {
    return undefined
  }
}

/** The bits asked of an address that sent `recent` reports in the last hour. */
export function bitsFor(powBits: number, recent: number): number {
  return Math.min(powBits + recent * 2, Math.max(MAX_BUSY_BITS, powBits))
}

export async function makeChallenge(db: D1Database, now: number, bits: number): Promise<Challenge> {
  const expiresAt = now + CHALLENGE_LIFE
  const body = new Uint8Array(25)
  body.set(crypto.getRandomValues(new Uint8Array(16)))
  new DataView(body.buffer).setBigInt64(16, BigInt(expiresAt))
  body[24] = bits
  const seal = (await mac(db, SEAL, body)).slice(0, 16)
  return { challenge: toBase64Url(concat(body, seal)), bits, expiresAt }
}

/** Checks the challenge and its answer, and marks it used. */
export async function takeChallenge(
  db: D1Database,
  now: number,
  challenge: string,
  nonce: number,
): Promise<Refused | undefined> {
  const token = fromBase64Url(challenge)
  if (!token || token.length !== 41) return 'unknown'
  const body = token.slice(0, 25)
  const expected = (await mac(db, SEAL, body)).slice(0, 16)
  // compared in constant time, the seal is what keeps challenges ours
  let differs = 0
  for (let i = 0; i < 16; i++) differs |= (expected[i] ?? 0) ^ (token[25 + i] ?? 0)
  if (differs !== 0) return 'unknown'
  const expiresAt = Number(new DataView(body.buffer).getBigInt64(16))
  const bits = body[24] ?? 0
  if (expiresAt <= now) return 'expired'
  if (!Number.isSafeInteger(nonce) || nonce < 0 || !(await meets(challenge, nonce, bits))) {
    return 'notSolved'
  }
  const added = await db
    .prepare('INSERT OR IGNORE INTO used_challenges (id, expires_at) VALUES (?1, ?2)')
    .bind(hex(body.slice(0, 16)), expiresAt)
    .run()
  return added.meta.changes === 0 ? 'used' : undefined
}
