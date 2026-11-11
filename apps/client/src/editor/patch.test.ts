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

  it('undo keeps a word another device put right before the word typed here', () => {
    const { editor } = editorWith('one\n\ntwo\n\nthree')
    caretAfter(editor, 'two')
    for (const key of ' alpha') editor.commands.insertContent(key)
    // the other device's caret sat at the end of "two" when " alpha" arrived
    patchMarkdown(editor, 'one\n\ntwo bravo alpha\n\nthree')
    expect(editor.getMarkdown()).toBe('one\n\ntwo bravo alpha\n\nthree')
    while (editor.can().undo()) editor.commands.undo()
    expect(editor.getMarkdown()).toBe('one\n\ntwo bravo\n\nthree')
    editor.commands.redo()
    expect(editor.getMarkdown()).toContain('two bravo alpha')
    editor.destroy()
  })

  it('undo keeps words another device put after, and before in another paragraph', () => {
    const { editor } = editorWith('one\n\ntwo\n\nthree')
    caretAfter(editor, 'two')
    for (const key of ' alpha') editor.commands.insertContent(key)
    patchMarkdown(editor, 'one bravo\n\ntwo alpha\n\nthree')
    patchMarkdown(editor, 'one bravo\n\ntwo alpha charlie\n\nthree')
    while (editor.can().undo()) editor.commands.undo()
    expect(editor.getMarkdown()).toBe('one bravo\n\ntwo charlie\n\nthree')
    editor.destroy()
  })

  it('text another device typed inside a word typed here goes where it was typed', () => {
    const { editor } = editorWith('start')
    caretAfter(editor, 'start')
    for (const key of ' abcdef') editor.commands.insertContent(key)
    patchMarkdown(editor, 'start abcXYZdef')
    expect(editor.getMarkdown()).toBe('start abcXYZdef')
    editor.destroy()
  })

  it('a space just typed at the end of a line stays, with the caret, when text arrives elsewhere', () => {
    // the space is saved, but Markdown read back has none at the end of a
    // line: the other device's text, read, comes without it
    for (const [incoming, reads] of [
      ['one\n\ntwo \n\nthree and more', 'one\n\ntwo next\n\nthree and more'],
      ['zero\n\none\n\ntwo \n\nthree', 'zero\n\none\n\ntwo next\n\nthree'],
      ['one\n\ntwo and \n\nthree', 'one\n\ntwo and next\n\nthree'],
      ['one\n\ntwo \n\nthree', 'one\n\ntwo next\n\nthree'],
    ] as const) {
      const { editor } = editorWith('one\n\ntwo\n\nthree')
      caretAfter(editor, 'two')
      editor.commands.insertContent(' ')
      expect(editor.getMarkdown()).toBe('one\n\ntwo \n\nthree')
      patchMarkdown(editor, incoming)
      editor.commands.insertContent('next')
      expect(editor.getMarkdown(), incoming).toBe(reads)
      editor.destroy()
    }
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
