// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import md from '../../../../CREDITS.md?raw'
import { CREDIT_GROUPS } from './credits'

// CREDITS.md at the repository root is the file people read on GitHub, the
// list in About must say the same

function parse(text: string) {
  const groups: { title: string; lines: string[] }[] = []
  for (const line of text.split(/\r?\n/)) {
    const head = /^### (.+)$/.exec(line)
    if (head) groups.push({ title: head[1] ?? '', lines: [] })
    else if (line.startsWith('- ') && groups.length) groups.at(-1)?.lines.push(line)
  }
  return groups
}

describe('credits', () => {
  it('are the same in CREDITS.md and in the app', () => {
    const expected = CREDIT_GROUPS.map((g) => ({
      title: g.title,
      lines: g.credits.map((c) => `- [${c.name}](${c.url}), ${c.use}. ${c.license}.`),
    }))
    expect(parse(md)).toEqual(expected)
  })

  it('have a name, a link, a use and a license each', () => {
    for (const c of CREDIT_GROUPS.flatMap((g) => g.credits)) {
      expect(c.name).not.toBe('')
      expect(c.url).toMatch(/^https:\/\//)
      expect(c.use).not.toBe('')
      expect(c.license).not.toBe('')
    }
  })
})
