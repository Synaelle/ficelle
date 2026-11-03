// SPDX-License-Identifier: AGPL-3.0-or-later
// An image dropped in text lands at the nearer end of a word, never inside it.

import { Editor } from '@tiptap/core'
import { describe, expect, it } from 'vitest'
import { createExtensions } from './extensions'
import { wordEdge } from './extensions/image'

function doc(markdown: string) {
  const element = document.createElement('div')
  const editor = new Editor({
    element,
    extensions: createExtensions(),
    content: markdown,
    contentType: 'markdown',
  })
  return editor.state.doc
}

describe('wordEdge', () => {
  // "hello world": the paragraph's text starts at position 1
  const d = doc('hello world')
  it('moves a point inside a word to its nearer end', () => {
    expect(wordEdge(d, 1 + 2)).toBe(1) // he|llo
    expect(wordEdge(d, 1 + 4)).toBe(1 + 5) // hell|o
    expect(wordEdge(d, 1 + 8)).toBe(1 + 6) // wo|rld
  })
  it('leaves a point between words as it is', () => {
    expect(wordEdge(d, 1 + 5)).toBe(1 + 5)
    expect(wordEdge(d, 1 + 6)).toBe(1 + 6)
    expect(wordEdge(d, 1)).toBe(1)
  })
})
