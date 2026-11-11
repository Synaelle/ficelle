// SPDX-License-Identifier: AGPL-3.0-or-later
// Who may read the inbox. It sits behind Cloudflare Access, which signs
// each request it lets through (`cf-access-jwt-assertion`, RS256). The
// token is checked here too, so the inbox stays closed if Access is ever
// missing in front of an address, workers.dev included. Without Access
// set up, only a local run opens it.

/** Set with `wrangler secret put`, absent in local runs. */
export interface AccessSecrets {
  /** The team's domain, `<team>.cloudflareaccess.com`. */
  ACCESS_TEAM_DOMAIN?: string
  /** The Access application's audience tag. */
  ACCESS_AUD?: string
}

const KEYS_LIFE = 60 * 60 * 1000

let keys: { team: string; at: number; list: JsonWebKey[] } | undefined

function base64UrlJson(part: string): unknown {
  return JSON.parse(
    new TextDecoder().decode(Uint8Array.fromBase64(part, { alphabet: 'base64url' })),
  ) as unknown
}

/**
 * The email in a valid Access token, `undefined` when it is not one: wrong
 * signature, audience or issuer, or expired.
 */
export async function verifyAccessToken(
  token: string,
  list: JsonWebKey[],
  team: string,
  aud: string,
  now: number,
): Promise<string | undefined> {
  const parts = token.split('.')
  if (parts.length !== 3) return undefined
  const [head, body, signature] = parts as [string, string, string]
  let header: { alg?: unknown; kid?: unknown }
  let claims: {
    aud?: unknown
    iss?: unknown
    exp?: unknown
    nbf?: unknown
    email?: unknown
  }
  let signed: Uint8Array<ArrayBuffer>
  try {
    header = base64UrlJson(head) as typeof header
    claims = base64UrlJson(body) as typeof claims
    signed = Uint8Array.fromBase64(signature, { alphabet: 'base64url' })
  } catch {
    return undefined
  }
  if (header.alg !== 'RS256') return undefined
  const jwk = list.find((k) => (k as { kid?: string }).kid === header.kid)
  if (!jwk) return undefined
  const key = await crypto.subtle.importKey(
    'jwk',
    jwk,
    { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' },
    false,
    ['verify'],
  )
  const valid = await crypto.subtle.verify(
    'RSASSA-PKCS1-v1_5',
    key,
    signed,
    new TextEncoder().encode(`${head}.${body}`),
  )
  if (!valid) return undefined
  const audiences = Array.isArray(claims.aud) ? claims.aud : [claims.aud]
  if (!audiences.includes(aud)) return undefined
  if (claims.iss !== `https://${team}`) return undefined
  const seconds = now / 1000
  if (typeof claims.exp !== 'number' || claims.exp <= seconds) return undefined
  if (typeof claims.nbf === 'number' && claims.nbf > seconds + 60) return undefined
  return typeof claims.email === 'string' ? claims.email : 'access'
}

async function teamKeys(team: string, now: number, fresh: boolean): Promise<JsonWebKey[]> {
  // a fresh look at most once a minute, an unknown key from anyone costs nothing more
  const life = fresh ? 60 * 1000 : KEYS_LIFE
  if (keys && keys.team === team && now - keys.at < life) return keys.list
  const response = await fetch(`https://${team}/cdn-cgi/access/certs`)
  if (!response.ok) throw new Error(`Access keys: ${response.status}`)
  const { keys: list } = await response.json<{ keys: JsonWebKey[] }>()
  keys = { team, at: now, list }
  return list
}

const LOCAL = new Set(['localhost', '127.0.0.1', '[::1]'])

/** Who is reading, `undefined` when the inbox stays closed to this request. */
export async function reader(
  request: Request,
  env: AccessSecrets,
  now: number,
): Promise<string | undefined> {
  const team = env.ACCESS_TEAM_DOMAIN?.trim()
    .replace(/^https:\/\//, '')
    .replace(/\/+$/, '')
  const aud = env.ACCESS_AUD?.trim()
  if (!team || !aud) {
    return LOCAL.has(new URL(request.url).hostname) ? 'local' : undefined
  }
  const token = request.headers.get('cf-access-jwt-assertion')
  if (!token) return undefined
  let list = await teamKeys(team, now, false)
  // Access turns its keys over now and then: an unknown key is looked up again
  if (!list.some((k) => (k as { kid?: string }).kid === tokenKid(token))) {
    list = await teamKeys(team, now, true)
  }
  return verifyAccessToken(token, list, team, aud, now)
}

function tokenKid(token: string): unknown {
  try {
    return (base64UrlJson(token.split('.')[0] ?? '') as { kid?: unknown }).kid
  } catch {
    return undefined
  }
}
