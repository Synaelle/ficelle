// SPDX-License-Identifier: AGPL-3.0-or-later
// Requests to the Worker as an app sends them, and the proof of work.

import { env } from 'cloudflare:test'

import { leadingZeroBits } from '../src/challenge'
import worker from '../src/index'

export const BASE = 'http://localhost'

let addresses = 0

/** A new address each time, so tests do not share their limits. */
export function freshIp(): string {
  addresses++
  return `198.18.${Math.floor(addresses / 250)}.${(addresses % 250) + 1}`
}

export async function call(
  path: string,
  init: RequestInit<IncomingRequestCfProperties> & { ip?: string; protocol?: string | null } = {},
): Promise<Response> {
  const headers = new Headers(init.headers)
  if (init.protocol !== null) headers.set('x-protocol-version', init.protocol ?? '1')
  headers.set('cf-connecting-ip', init.ip ?? '198.51.100.7')
  const request = new Request<unknown, IncomingRequestCfProperties>(`${BASE}${path}`, {
    ...init,
    headers,
  })
  return worker.fetch(request, env)
}

export function post(path: string, body: unknown, ip?: string): Promise<Response> {
  return call(path, {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'content-type': 'application/json' },
    ip,
  })
}

export async function solve(challenge: string, bits: number): Promise<number> {
  const prefix = new TextEncoder().encode(challenge)
  const data = new Uint8Array(prefix.length + 8)
  data.set(prefix)
  const view = new DataView(data.buffer)
  for (let nonce = 0; ; nonce++) {
    view.setBigUint64(prefix.length, BigInt(nonce), true)
    const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', data))
    if (leadingZeroBits(hash) >= bits) return nonce
  }
}

export async function solved(ip: string): Promise<{ challenge: string; nonce: number }> {
  const response = await post('/api/v1/feedback/challenge', {}, ip)
  if (response.status !== 200) throw new Error(`challenge: ${response.status}`)
  const { challenge, bits } = await response.json<{ challenge: string; bits: number }>()
  return { challenge, nonce: await solve(challenge, bits) }
}

/** A PNG header of the given size, enough for the checks. */
export function pngOf(width: number, height: number): Uint8Array {
  const bytes = new Uint8Array(33)
  bytes.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d])
  bytes.set(new TextEncoder().encode('IHDR'), 12)
  const view = new DataView(bytes.buffer)
  view.setUint32(16, width)
  view.setUint32(20, height)
  bytes.set([8, 6, 0, 0, 0], 24)
  return bytes
}

export function base64(bytes: Uint8Array): string {
  return bytes.toBase64()
}

export async function receiptOf(secret: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(secret))
  return new Uint8Array(digest).toHex()
}

/** Sends a whole report, the challenge solved first. */
export async function sendReport(
  fields: Record<string, unknown>,
  ip = freshIp(),
): Promise<Response> {
  const proof = await solved(ip)
  return post(
    '/api/v1/feedback',
    {
      ...proof,
      install: '0123456789abcdef0123456789abcdef',
      about: 'The table bar moves after a resize',
      message: 'After resizing the window the table bar sits below the wrong table.',
      version: '0.6.0-beta.1',
      platform: 'linux',
      ...fields,
    },
    ip,
  )
}
