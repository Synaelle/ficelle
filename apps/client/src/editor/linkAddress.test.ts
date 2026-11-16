// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { linkAddress } from './linkAddress'

describe('linkAddress', () => {
  it('puts https:// before a bare domain', () => {
    expect(linkAddress('example.org/hours')).toBe('https://example.org/hours')
    expect(linkAddress(' www.example.org ')).toBe('https://www.example.org')
    expect(linkAddress('notes.example.org:8080/a?b#c')).toBe('https://notes.example.org:8080/a?b#c')
  })

  it('leaves addresses with a scheme, paths and anchors alone', () => {
    for (const kept of [
      'https://example.org',
      'http://a.b',
      'mailto:me@example.org',
      'tel:+33123',
      '#plan',
      '/a/b',
      './c.md',
      'Plan',
    ])
      expect(linkAddress(kept)).toBe(kept)
  })

  it('makes a mail address a mailto link', () => {
    expect(linkAddress('me@example.org')).toBe('mailto:me@example.org')
  })
})
