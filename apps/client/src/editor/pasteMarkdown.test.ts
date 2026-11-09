// SPDX-License-Identifier: AGPL-3.0-or-later
import { Editor } from '@tiptap/core'
import { describe, expect, it } from 'vitest'
import { createExtensions } from './extensions'
import { looksLikeMarkdown } from './pasteMarkdown'

describe('looksLikeMarkdown', () => {
  it.each([
    '# A title',
    'Intro\n\n## Part two',
    '- one\n- two',
    '1. first\n2. second',
    '> quoted',
    '```js\nlet a\n```',
    '| a | b |\n| - | - |',
    'some **bold** words',
    'a ~~struck~~ word',
    'run `pnpm lint` first',
    'see [the docs](https://example.org)',
    'see [[Garden log]]',
    '---',
  ])('reads %j as Markdown', (text) => {
    expect(looksLikeMarkdown(text)).toBe(true)
  })

  it.each([
    'Just a sentence.',
    'Two lines\nof plain text',
    'A price of 5 * 3 = 15',
    '#hashtag without a space',
    'an email@example.org and a path/to/file',
  ])('leaves %j as text', (text) => {
    expect(looksLikeMarkdown(text)).toBe(false)
  })
})

describe('Markdown inserted into a note', () => {
  it('comes out formatted', () => {
    const editor = new Editor({
      element: document.createElement('div'),
      extensions: createExtensions(),
      content: '',
      contentType: 'markdown',
    })
    editor.commands.insertContent('# Title\n\n- **one**\n- two', { contentType: 'markdown' })
    const html = editor.getHTML()
    editor.destroy()
    expect(html).toContain('<h1')
    expect(html).toContain('<ul')
    expect(html).toContain('<strong>one</strong>')
  })
})
