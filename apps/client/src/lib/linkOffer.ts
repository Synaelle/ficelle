// SPDX-License-Identifier: AGPL-3.0-or-later
// What the QR code on a sync server's /link page holds: the server's address,
// /link, then `#offer=` and a one-time token. The phone reads both from it.

/** The server and the token in a scanned QR code, null for any other text. */
export function parseOffer(text: string): { server: string; offer: string } | null {
  const found = /^(https?:\/\/[^\s#?]+?)\/link#offer=(lo_[A-Za-z0-9_-]{20,100})$/.exec(text.trim())
  const [, server, offer] = found ?? []
  return server && offer ? { server, offer } : null
}
