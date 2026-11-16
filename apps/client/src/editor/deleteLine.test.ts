// SPDX-License-Identifier: AGPL-3.0-or-later
// Ctrl+K through the real schema: one line goes, never more, and no empty
// quote, bullet or list stays behind.

import { Editor } from '@tiptap/core'
import { TextSelection } from '@tiptap/pm/state'
import { describe, expect, it } from 'vitest'
import { deleteLine } from './deleteLine'
import { createExtensions } from './extensions'

function editorWith(markdown: string) {
  return new Editor({
    element: document.createElement('div'),
    extensions: createExtensions(),
    content: markdown,
    contentType: 'markdown',
  })
}

/** The caret at the start of the first text `word` is in. */
function caretAt(editor: Editor, word: string) {
  let at = -1
  editor.state.doc.descendants((node, pos) => {
    if (at < 0 && node.isText && node.text?.includes(word)) at = pos + node.text.indexOf(word)
    return at < 0
  })
  editor.view.dispatch(editor.state.tr.setSelection(TextSelection.create(editor.state.doc, at)))
}

const text = (editor: Editor) => editor.getMarkdown().trim()

describe('deleteLine', () => {
  it('takes one line of a code block, not the block', () => {
    const editor = editorWith('```js\nalpha()\nbeta()\ngamma()\ndelta()\n```\n\nAfter')
    caretAt(editor, 'gamma')
    deleteLine(editor)
    expect(text(editor)).toBe('```js\nalpha()\nbeta()\ndelta()\n```\n\nAfter')
    caretAt(editor, 'delta')
    deleteLine(editor)
    expect(text(editor)).toBe('```js\nalpha()\nbeta()\n```\n\nAfter')
  })

  it('takes a one-line code block whole', () => {
    const editor = editorWith('Before\n\n```\nonly()\n```\n\nAfter')
    caretAt(editor, 'only')
    deleteLine(editor)
    expect(text(editor)).toBe('Before\n\nAfter')
  })

  it('takes a one-line quote whole, and the next Ctrl+K the next line only', () => {
    const editor = editorWith('Before\n\n> Remember the umbrella\n\nLast line')
    caretAt(editor, 'Remember')
    deleteLine(editor)
    expect(text(editor)).toBe('Before\n\nLast line')
  })

  it('takes a single bullet with its list, one bullet of many alone', () => {
    const one = editorWith('Before\n\n- Lonely\n\nAfter')
    caretAt(one, 'Lonely')
    deleteLine(one)
    expect(text(one)).toBe('Before\n\nAfter')

    const many = editorWith('- one\n- two\n- three')
    caretAt(many, 'two')
    deleteLine(many)
    expect(text(many)).toBe('- one\n- three')
  })

  it('takes one line of a quote that has several', () => {
    const editor = editorWith('> first\n>\n> second')
    caretAt(editor, 'second')
    deleteLine(editor)
    expect(text(editor)).toBe('> first')
  })
})
