// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { isMarkdownFile, parseMarkdownFile } from './importing'

describe('isMarkdownFile', () => {
  it('accepts the usual markdown extensions only', () => {
    for (const name of ['a.md', 'b.MD', 'c.markdown', 'd.mdown', 'e.mkd']) {
      expect(isMarkdownFile({ name })).toBe(true)
    }
    for (const name of ['a.txt', 'b.png', 'md', 'notes.md.zip']) {
      expect(isMarkdownFile({ name })).toBe(false)
    }
  })
})

describe('parseMarkdownFile', () => {
  it('uses the file name when there is no heading', () => {
    expect(parseMarkdownFile('Garden log.md', 'Some text\n')).toEqual({
      title: 'Garden log',
      body: 'Some text\n',
      tags: [],
    })
  })

  it('moves a leading level one heading into the title', () => {
    const note = parseMarkdownFile('x.md', '\n# Garden log #\n\nFirst line\n## Kept\n')
    expect(note.title).toBe('Garden log')
    expect(note.body).toBe('First line\n## Kept\n')
  })

  it('keeps a level two heading or a later heading in the body', () => {
    expect(parseMarkdownFile('x.md', '## Sub\ntext').title).toBe('x')
    expect(parseMarkdownFile('x.md', 'intro\n# Late').body).toBe('intro\n# Late')
  })

  it('reads title and tags from front matter and drops the block', () => {
    const text =
      '---\ntitle: "Trip plan"\ntags: [travel, #2026]\ndate: 2026-01-02\n---\n# Day one\nGo'
    expect(parseMarkdownFile('x.md', text)).toEqual({
      title: 'Trip plan',
      body: '# Day one\nGo',
      tags: ['travel', '2026'],
    })
  })

  it('reads front matter tags written as a list', () => {
    const text = '---\ntags:\n  - one\n  - "two"\naliases: x\n---\nbody'
    expect(parseMarkdownFile('Note.md', text)).toEqual({
      title: 'Note',
      body: 'body',
      tags: ['one', 'two'],
    })
  })

  it('normalises windows line endings and a byte order mark', () => {
    expect(parseMarkdownFile('x.md', '\uFEFF# Title\r\nline\r\n')).toEqual({
      title: 'Title',
      body: 'line\n',
      tags: [],
    })
  })
})
