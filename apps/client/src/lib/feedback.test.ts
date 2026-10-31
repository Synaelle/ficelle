// SPDX-License-Identifier: AGPL-3.0-or-later
// What a feedback report's logs may carry, this install's id, the draft.

import { describe, expect, it } from 'vitest'
import {
  captureLogs,
  contactOk,
  EMPTY_DRAFT,
  installId,
  loadDraft,
  MAX_LINES,
  MAX_LOGS,
  recentLogs,
  reportLogs,
  saveDraft,
  scrub,
} from './feedback'
import { prefs } from './prefs.svelte'

describe('scrub', () => {
  it('takes out home folders, emails and note titles', () => {
    const text = [
      'open C:\\Users\\Camille Durand\\AppData\\Roaming\\vault.db failed',
      'read /home/camille/notes/x and /Users/camille/Library/y',
      'android /data/user/0/com.example.notes/files/vault',
      'mail kim@example.org refused',
      'note "Medical results (2026)" failed to save, and medical RESULTS (2026) again',
      'title Tax return 2025 [draft] gone',
    ].join('\n')
    const out = scrub(text, ['Medical results (2026)', 'Tax return 2025 [draft]', 'ab', 'Tax'])
    for (const secret of [
      'Camille',
      'camille',
      'kim@example.org',
      'Medical',
      'medical',
      'Tax return',
    ]) {
      expect(out).not.toContain(secret)
    }
    expect(out).toContain('~\\AppData\\Roaming\\vault.db')
    expect(out).toContain('~/notes/x')
    expect(out).toContain('[email]')
    expect(out.match(/\[title\]/g)?.length).toBe(3)
    // two letters is too short to be told from ordinary words
    expect(scrub('about tabs', ['ab'])).toBe('about tabs')
  })

  it('keeps the newest lines within the caps, cleaned', () => {
    captureLogs()
    captureLogs()
    const quiet = { error: console.error, warn: console.warn }
    console.error('the note "Groceries for Sunday" could not be read')
    console.warn(new Error('sync stopped for kim@example.org'))
    window.dispatchEvent(
      new ErrorEvent('error', { message: 'boom', filename: 'app.js', lineno: 3 }),
    )
    const lines = recentLogs()
    expect(lines.at(-3)).toMatch(/^\d\d:\d\d:\d\d error: the note/)
    expect(lines.at(-2)).toMatch(/warn: Error: sync stopped/)
    expect(lines.at(-1)).toContain('boom at app.js:3')
    const logs = reportLogs(['Groceries for Sunday'])
    expect(logs).not.toContain('Groceries')
    expect(logs).not.toContain('kim@example.org')
    for (let i = 0; i < MAX_LINES + 10; i++) quiet.error(`line ${i} ${'x'.repeat(2_000)}`)
    expect(recentLogs()).toHaveLength(MAX_LINES)
    // each line is cut at 500 characters, so 50 lines stay under the cap
    expect(new TextEncoder().encode(reportLogs([])).length).toBeLessThanOrEqual(MAX_LOGS)
    expect(recentLogs().at(-1)).toMatch(/line 59 x+$/)
  })
})

describe('install id', () => {
  it('is made once, random, and replaced when broken', () => {
    prefs.feedbackInstall = ''
    const id = installId()
    expect(id).toMatch(/^[0-9a-f]{32}$/)
    expect(installId()).toBe(id)
    prefs.feedbackInstall = 'not an id'
    expect(installId()).not.toBe(id)
  })
})

describe('contactOk', () => {
  it('takes an email address or a Discord name, as the server does', () => {
    for (const ok of [
      '',
      'kim@example.org',
      ' Kim@Example.ORG ',
      '@Synaelle',
      'kim.dev_2',
      'OldName#0042',
    ]) {
      expect(contactOk(ok), ok).toBe(true)
    }
    for (const bad of [
      'a@b',
      'a@b.c@d.e',
      '@a@b.org',
      'x',
      'two words',
      'name!',
      'n#12',
      'a'.repeat(33),
    ]) {
      expect(contactOk(bad), bad).toBe(false)
    }
  })
})

describe('draft', () => {
  it('keeps the words of a report not sent, and forgets an empty one', () => {
    saveDraft({ ...EMPTY_DRAFT, about: 'The sidebar flickers', logs: false })
    expect(loadDraft()).toEqual({ ...EMPTY_DRAFT, about: 'The sidebar flickers', logs: false })
    // a draft from before 0.2.0 keeps its words, its category and email go
    localStorage.setItem(
      'feedback-draft',
      JSON.stringify({ category: 'bug', message: 'old words', email: 'k@x.org', logs: false }),
    )
    expect(loadDraft()).toEqual({ about: '', message: 'old words', contact: '', logs: false })
    saveDraft({ ...EMPTY_DRAFT, message: '   ' })
    expect(loadDraft()).toEqual(EMPTY_DRAFT)
    saveDraft(null)
    expect(loadDraft()).toEqual(EMPTY_DRAFT)
  })
})

describe('scrub, at random', () => {
  // xorshift, a failing seed fails the same way every time
  function rng(seed: number) {
    let x = seed | 1
    return (n: number) => {
      x ^= x << 13
      x ^= x >>> 17
      x ^= x << 5
      return (x >>> 0) % n
    }
  }
  // letters, digits, accents, spaces and the characters regular expressions
  // treat apart, never the markers' own brackets
  const PIECES = [
    'a',
    'B',
    'é',
    'Ö',
    '7',
    ' ',
    '.',
    '*',
    '+',
    '?',
    '(',
    ')',
    '$',
    '^',
    '|',
    '\\',
    '{',
    '}',
    '漢',
  ]

  it('leaves no title, user folder or email behind, whatever they hold', () => {
    for (let seed = 1; seed <= 200; seed++) {
      const below = rng(seed)
      const word = (min: number, max: number) =>
        Array.from({ length: min + below(max - min + 1) }, () => PIECES[below(PIECES.length)]).join(
          '',
        )
      const titles = Array.from({ length: 1 + below(8) }, () => word(3, 20)).filter(
        (t) =>
          t.trim().length >= 3 && !'[title][email]~'.toLowerCase().includes(t.trim().toLowerCase()),
      )
      const user = `${word(1, 6).replace(/[\\/]/g, '')}x ${word(1, 6).replace(/[\\/]/g, '')}y`
      const lines = Array.from({ length: 1 + below(10) }, () => {
        const title = titles[below(titles.length)] ?? 'none'
        const shout = below(2) ? title.toUpperCase() : title
        return [
          `failed "${shout}" in C:\\Users\\${user}\\AppData\\x`,
          `see /home/${user.replace(/ /g, '')}/notes about ${title}`,
          `${word(1, 5).replace(/\W/g, '')}a@mail.example.org wrote ${title}`,
        ][below(3)]
      })
      const out = scrub(lines.join('\n'), titles)
      for (const title of titles) {
        expect(out.toLowerCase(), `seed ${seed}: ${JSON.stringify(title)}`).not.toContain(
          title.trim().toLowerCase(),
        )
      }
      expect(out, `seed ${seed}`).not.toContain(user)
      expect(out, `seed ${seed}`).not.toMatch(/@mail\.example\.org/)
    }
  })
})
