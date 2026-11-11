// SPDX-License-Identifier: AGPL-3.0-or-later
// Another device's text patched into the real editor: only what differs
// changes, the caret stays on the words it was on, the patch is not saved
// back as a local edit and is not undone by the person's own undo.

import { Editor } from '@tiptap/core'
import { TextSelection } from '@tiptap/pm/state'
import { describe, expect, it } from 'vitest'
import { createExtensions } from './extensions'
import { patchMarkdown, patchTransaction, REMOTE } from './patch'

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

  it('text changed above and under the caret at once leaves it where it is', () => {
    const { editor } = editorWith('alpha line\n\nbravo line\n\ncharlie line')
    caretAfter(editor, 'bravo line')
    editor.commands.insertContent(' tb0')
    expect(
      patchMarkdown(editor, 'alpha line ta0 ta1\n\nbravo line tb0\n\ncharlie line tc0 tc1'),
    ).toBe(true)
    editor.commands.insertContent(' tb1')
    expect(editor.getMarkdown()).toBe(
      'alpha line ta0 ta1\n\nbravo line tb0 tb1\n\ncharlie line tc0 tc1',
    )
    // and with the caret's own line changed too, before the caret
    patchMarkdown(editor, 'new first\n\nalpha\n\nBravo line tb0 tb1\n\ncharlie\n\n- a\n- b')
    editor.commands.insertContent(' tb2')
    expect(editor.getMarkdown().trimEnd()).toBe(
      'new first\n\nalpha\n\nBravo line tb0 tb1 tb2\n\ncharlie\n\n- a\n- b',
    )
    editor.destroy()
  })

  it('items changed around the caret in a list, and cells in a table, leave it where it is', () => {
    const { editor } = editorWith('- one\n- two\n- three\n\n| a | b |\n| --- | --- |\n| c | d |')
    caretAfter(editor, 'two')
    patchMarkdown(editor, '- one!\n- two\n- three!\n- four\n\n| a | b |\n| --- | --- |\n| c | d |')
    editor.commands.insertContent(' mine')
    caretAfter(editor, 'c')
    patchMarkdown(
      editor,
      '- one!\n- two mine\n- three!\n- four\n\n| a2 | b |\n| --- | --- |\n| c | d2 |\n| e | f |',
    )
    editor.commands.insertContent('!')
    expect(editor.getMarkdown().trimEnd()).toBe(
      '- one!\n- two mine\n- three!\n- four\n\n| a2 | b |\n| --- | --- |\n| c! | d2 |\n| e | f |',
    )
    editor.destroy()
  })

  it('any text becomes any other', () => {
    const texts = [
      '# Plan\n\nfirst\n\nsecond',
      'first\n\n- a\n- b\n\nsecond\n\nthird',
      '> quote\n\n```js\ncode\n```\n\nfirst',
      'only',
      '- [ ] task\n- [x] done\n\n| a | b |\n| --- | --- |\n| c | d |\n\nsecond',
      'second\n\nfirst\n\nsecond\n\nfirst',
    ]
    for (const from of texts)
      for (const to of texts) {
        const { editor } = editorWith(from)
        const next = editor.schema.nodeFromJSON(editor.markdown?.parse(to))
        const tr = patchTransaction(editor.state.tr, editor.state.doc, next)
        expect((tr?.doc ?? editor.state.doc).eq(next), `${from} to ${to}`).toBe(true)
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
