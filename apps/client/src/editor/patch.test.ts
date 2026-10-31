// SPDX-License-Identifier: AGPL-3.0-or-later
// Another device's text patched into the real editor: only what differs
// changes, the caret stays on the words it was on, the patch is not saved
// back as a local edit and is not undone by the person's own undo.

import { Editor } from '@tiptap/core'
import { TextSelection } from '@tiptap/pm/state'
import { describe, expect, it } from 'vitest'
import { createExtensions } from './extensions'
import { patchMarkdown, REMOTE } from './patch'

function editorWith(markdown: string) {
  const updates: boolean[] = []
  const editor = new Editor({
    element: document.createElement('div'),
    extensions: createExtensions(),
    content: markdown,
    contentType: 'markdown',
    onUpdate: ({ transaction }) => updates.push(Boolean(transaction.getMeta(REMOTE))),
  })
  return { editor, updates }
}

/** Puts the caret just after `word`. */
function caretAfter(editor: Editor, word: string) {
  let at = -1
  editor.state.doc.descendants((node, pos) => {
    if (at < 0 && node.isText && node.text?.includes(word)) {
      at = pos + (node.text.indexOf(word) + word.length)
    }
  })
  editor.view.dispatch(editor.state.tr.setSelection(TextSelection.create(editor.state.doc, at)))
}

const textBeforeCaret = (editor: Editor) => {
  const { from } = editor.state.selection
  return editor.state.doc.textBetween(Math.max(0, from - 5), from)
}

describe('remote text in the editor', () => {
  it('a line added above keeps the caret on its words', () => {
    const { editor, updates } = editorWith('# Plan\n\nfirst line\n\nsecond line')
    caretAfter(editor, 'second')
    expect(textBeforeCaret(editor)).toBe('econd')
    expect(patchMarkdown(editor, '# Plan\n\nadded elsewhere\n\nfirst line\n\nsecond line')).toBe(
      true,
    )
    expect(editor.getMarkdown()).toContain('added elsewhere')
    expect(textBeforeCaret(editor)).toBe('econd')
    expect(updates.at(-1)).toBe(true)
    editor.destroy()
  })

  it('an edit inside the same paragraph after the caret leaves it alone', () => {
    const { editor } = editorWith('one two three')
    caretAfter(editor, 'one')
    patchMarkdown(editor, 'one two and a half three')
    expect(editor.getMarkdown()).toBe('one two and a half three')
    expect(textBeforeCaret(editor)).toBe('one')
    editor.destroy()
  })

  it('the same text changes nothing', () => {
    const { editor, updates } = editorWith('# Same\n\n- a\n- b')
    expect(patchMarkdown(editor, '# Same\n\n- a\n- b')).toBe(false)
    expect(updates).toEqual([])
    editor.destroy()
  })

  it('undo takes back the own edit, not the remote one', () => {
    const { editor } = editorWith('mine')
    caretAfter(editor, 'mine')
    editor.commands.insertContent(' typed')
    patchMarkdown(editor, 'theirs\n\nmine typed')
    editor.commands.undo()
    expect(editor.getMarkdown()).toBe('theirs\n\nmine')
    editor.destroy()
  })
})
