// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { parseOffer } from './linkOffer'

const TOKEN = 'lo_q2p8Zt4cWnD0kLmV7yB3xR9sA1eH6uJfGiOaNbXcYd'

describe('parseOffer', () => {
  it('reads the server and the token, under a path too', () => {
    expect(parseOffer(`https://notes.example.org/link#offer=${TOKEN}`)).toEqual({
      server: 'https://notes.example.org',
      offer: TOKEN,
    })
    expect(parseOffer(` https://example.org/notes/link#offer=${TOKEN}\n`)?.server).toBe(
      'https://example.org/notes',
    )
  })

  it('takes nothing else', () => {
    for (const text of [
      'https://example.org',
      `https://example.org/link#code=${TOKEN}`,
      `https://example.org/link#offer=ac_${TOKEN.slice(3)}`,
      `https://example.org/link#offer=${TOKEN}&x=1`,
      `ftp://example.org/link#offer=${TOKEN}`,
      `https://example.org/link?x=1#offer=${TOKEN}`,
      'WIFI:S:home;T:WPA;P:secret;;',
    ]) {
      expect(parseOffer(text), text).toBeNull()
    }
  })
})
