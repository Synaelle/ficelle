// SPDX-License-Identifier: AGPL-3.0-or-later
// The Worker's own random key, kept in D1 and made on first use: it seals
// challenges and hashes addresses. Addresses are kept only as that hash.

const encoder = new TextEncoder()

let cached: Promise<CryptoKey> | undefined

export function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

function fromHex(text: string): Uint8Array {
  const bytes = new Uint8Array(text.length / 2)
  for (let i = 0; i < bytes.length; i++) bytes[i] = parseInt(text.slice(i * 2, i * 2 + 2), 16)
  return bytes
}

export function concat(...parts: Uint8Array[]): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.byteLength, 0))
  let at = 0
  for (const part of parts) {
    out.set(part, at)
    at += part.byteLength
  }
  return out
}

async function load(db: D1Database): Promise<CryptoKey> {
  const fresh = hex(crypto.getRandomValues(new Uint8Array(32)))
  // two first requests at once agree on whichever was written first
  await db
    .prepare("INSERT OR IGNORE INTO meta (key, value) VALUES ('secret', ?1)")
    .bind(fresh)
    .run()
  const row = await db
    .prepare("SELECT value FROM meta WHERE key = 'secret'")
    .first<{ value: string }>()
  if (!row) throw new Error('the secret was not kept')
  return crypto.subtle.importKey(
    'raw',
    fromHex(row.value),
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign'],
  )
}

function secret(db: D1Database): Promise<CryptoKey> {
  cached ??= load(db).catch((err: unknown) => {
    cached = undefined
    throw err
  })
  return cached
}

export async function mac(db: D1Database, label: string, data: Uint8Array): Promise<Uint8Array> {
  const signed = await crypto.subtle.sign(
    'HMAC',
    await secret(db),
    concat(encoder.encode(label), data),
  )
  return new Uint8Array(signed)
}

export async function sha256(data: Uint8Array<ArrayBuffer>): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest('SHA-256', data))
}

export async function sha256Hex(text: string): Promise<string> {
  return hex(await sha256(concat(encoder.encode(text))))
}

function ipv4(text: string): number[] | undefined {
  const parts = text.split('.')
  if (parts.length !== 4 || !parts.every((p) => /^\d{1,3}$/.test(p) && Number(p) < 256)) {
    return undefined
  }
  return parts.map(Number)
}

function ipv6(text: string): number[] | undefined {
  const address = text.split('%')[0] ?? ''
  const halves = address.split('::')
  if (halves.length > 2) return undefined
  const groups = (half: string) => (half === '' ? [] : half.split(':'))
  const head = groups(halves[0] ?? '')
  const tail = groups(halves[1] ?? '')
  const words: number[] = []
  const read = (list: string[]): boolean => {
    for (const [n, group] of list.entries()) {
      if (n === list.length - 1 && group.includes('.')) {
        const v4 = ipv4(group)
        if (!v4) return false
        const [a = 0, b = 0, c = 0, d = 0] = v4
        words.push((a << 8) | b, (c << 8) | d)
      } else if (/^[0-9a-f]{1,4}$/i.test(group)) {
        words.push(parseInt(group, 16))
      } else {
        return false
      }
    }
    return true
  }
  if (!read(head)) return undefined
  const before = words.length
  if (!read(tail)) return undefined
  const missing = 8 - words.length
  if (halves.length === 1 ? missing !== 0 : missing < 1) return undefined
  words.splice(before, 0, ...new Array<number>(missing).fill(0))
  return words.flatMap((w) => [w >> 8, w & 0xff])
}

/**
 * The address as kept: a keyed hash of it, of its first 64 bits for IPv6
 * (one home or one server has a whole /64 to move around in).
 */
export async function addressKey(db: D1Database, ip: string): Promise<string> {
  let prefix: Uint8Array
  const v4 = ipv4(ip)
  const v6 = v4 ? undefined : ipv6(ip)
  if (v4) {
    prefix = new Uint8Array(v4)
  } else if (v6) {
    const mapped = v6.slice(0, 10).every((b) => b === 0) && v6[10] === 0xff && v6[11] === 0xff
    prefix = new Uint8Array(mapped ? v6.slice(12) : v6.slice(0, 8))
  } else {
    prefix = encoder.encode(ip)
  }
  return hex((await mac(db, 'feedback address\0', prefix)).slice(0, 16))
}

/** A time-ordered id, a UUID v7 as 32 hex. */
export function newId(now: number): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  let time = now
  for (let i = 5; i >= 0; i--) {
    bytes[i] = time % 256
    time = Math.floor(time / 256)
  }
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x70
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80
  return hex(bytes)
}
