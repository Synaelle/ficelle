// SPDX-License-Identifier: AGPL-3.0-or-later
// Answers in the server's form: errors as `{ error, message }` with the
// right status, the protocol header every api call names, bodies read with
// a cap.

/** The protocol the apps speak, as in `crates/protocol`. */
export const PROTOCOL_VERSION = 1
export const MIN_PROTOCOL_VERSION = 1
export const PROTOCOL_HEADER = 'x-protocol-version'

export function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

export function apiError(
  status: number,
  code: string,
  message: string,
  retryAfter?: number,
): Response {
  const response = json(
    retryAfter === undefined
      ? { error: code, message }
      : { error: code, message, retryAfterSeconds: retryAfter },
    status,
  )
  if (retryAfter !== undefined) response.headers.set('retry-after', String(retryAfter))
  return response
}

/**
 * An empty answer, never compressed. Cloudflare's edge compresses a response
 * without a content type for a client that accepts gzip, which marks an
 * empty body as gzip, and the apps' client fails to decompress nothing.
 * Naming the encoding keeps it as it is.
 */
export function noContent(headers?: Record<string, string>): Response {
  return new Response(null, {
    status: 204,
    headers: { ...headers, 'content-encoding': 'identity' },
    encodeBody: 'manual',
  })
}

export function invalid(message: string): Response {
  return apiError(400, 'invalid', message)
}

export function rateLimited(seconds: number): Response {
  return apiError(429, 'rateLimited', 'too many reports, try again later', seconds)
}

/** A refusal when the request names no protocol or one this side does not speak. */
export function checkProtocol(request: Request): Response | undefined {
  const text = request.headers.get(PROTOCOL_HEADER)?.trim() ?? ''
  const version = /^\d{1,9}$/.test(text) ? Number(text) : undefined
  if (version === undefined) {
    return apiError(400, 'protocolMissing', `send the ${PROTOCOL_HEADER} header`)
  }
  const refused = (code: string, side: string) =>
    apiError(
      426,
      code,
      `protocol ${version} is not supported, this server speaks ${MIN_PROTOCOL_VERSION} to ${PROTOCOL_VERSION}: update the ${side}`,
    )
  if (version < MIN_PROTOCOL_VERSION) return refused('updateApp', 'app')
  if (version > PROTOCOL_VERSION) return refused('updateServer', 'server')
  return undefined
}

/** The body, or `undefined` once it passes `max` bytes. */
export async function readCapped(request: Request, max: number): Promise<Uint8Array | undefined> {
  const declared = Number(request.headers.get('content-length') ?? 'NaN')
  if (declared > max) return undefined
  if (!request.body) return new Uint8Array()
  const parts: Uint8Array[] = []
  let size = 0
  const reader = request.body.getReader()
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    size += value.byteLength
    if (size > max) {
      await reader.cancel()
      return undefined
    }
    parts.push(value)
  }
  const body = new Uint8Array(size)
  let at = 0
  for (const part of parts) {
    body.set(part, at)
    at += part.byteLength
  }
  return body
}
