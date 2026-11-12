// SPDX-License-Identifier: AGPL-3.0-or-later
import { Editor } from '@tiptap/core'
import { describe, expect, it } from 'vitest'
import { asWritten } from './asWritten'
import { createExtensions } from './extensions'

/** A note as another app wrote it. */
const STORED =
  'Heading\n=======\n\nintro\n\n* apples\n* pears\n\n__bold words__ and *some* more\n\n\n\n3. three\n4. four\n\n| item | n |\n|---|---|\n| rice | 2 |\n\nafter\n'

function editorWith(markdown: string) {
  return new Editor({
    element: document.createElement('div'),
    extensions: createExtensions(),
    content: markdown,
    contentType: 'markdown',
  })
}

/** Types `text` at the end of the block that reads `block`. */
function typeAfter(editor: Editor, block: string, text: string) {
  let at = -1
  editor.state.doc.descendants((node, pos) => {
    if (at < 0 && node.isTextblock && node.textContent === block) at = pos + node.nodeSize - 1
  })
  expect(at, `a block reading ${block}`).toBeGreaterThan(0)
  editor.commands.insertContentAt(at, text)
}

describe('saving a note written in another form', () => {
  it('nothing changed saves nothing new', () => {
    const editor = editorWith(STORED)
    const before = editor.getMarkdown()
    expect(before).not.toBe(STORED)
    expect(asWritten(STORED, before, before)).toBe(STORED)
    editor.destroy()
  })

  it('a word typed changes its line and no other', () => {
    const editor = editorWith(STORED)
    const before = editor.getMarkdown()
    typeAfter(editor, 'intro', ' x')
    expect(asWritten(STORED, before, editor.getMarkdown())).toBe(STORED.replace('intro', 'intro x'))
    typeAfter(editor, 'after', ' y')
    const saved = asWritten(STORED, before, editor.getMarkdown())
    expect(saved).toBe(STORED.replace('intro', 'intro x').replace('after', 'after y'))
    // and the saved text reads as the note in the editor
    const again = editorWith(saved)
    expect(again.getMarkdown()).toBe(editor.getMarkdown())
    again.destroy()
    editor.destroy()
  })

  it('a line typed in takes the editor form, the rest stays', () => {
    const editor = editorWith(STORED)
    const before = editor.getMarkdown()
    typeAfter(editor, 'bold words and some more', '!')
    const saved = asWritten(STORED, before, editor.getMarkdown())
    expect(saved).toContain('**bold words** and *some* more!')
    expect(saved).toContain('Heading\n=======\n')
    expect(saved).toContain('* apples\n* pears')
    expect(saved).toContain('more!\n\n\n\n3. three')
    editor.destroy()
  })

  it('two devices typing in different lines of it save changes that do not meet', () => {
    const one = editorWith(STORED)
    const two = editorWith(STORED)
    const before = one.getMarkdown()
    typeAfter(one, 'intro', ' a0')
    typeAfter(two, 'after', ' b0')
    const first = asWritten(STORED, before, one.getMarkdown())
    const second = asWritten(STORED, before, two.getMarkdown())
    const changed = (saved: string) =>
      saved.split('\n').filter((line, at) => line !== STORED.split('\n')[at])
    expect(changed(first)).toEqual(['intro a0'])
    expect(changed(second)).toEqual(['after b0'])
    one.destroy()
    two.destroy()
  })

  it('a table cell typed in rewrites the table whole, and nothing else', () => {
    const editor = editorWith(STORED)
    const before = editor.getMarkdown()
    typeAfter(editor, 'rice', ' and beans')
    const saved = asWritten(STORED, before, editor.getMarkdown())
    const rows = saved.split('\n').filter((line) => line.startsWith('|'))
    expect(rows).toEqual(['| item | n |', '| --- | --- |', '| rice and beans | 2 |'])
    expect(saved.replace(/\|.*\|\n/g, '')).toBe(STORED.replace(/\|.*\|\n/g, ''))
    editor.destroy()
  })

  it('two cells typed in on two devices change one row each', () => {
    const table = '| item | qty |\n| --- | :---: |\n| rice | 2 |\n| beans | |\n| a \\| b | 3 |'
    const one = editorWith(table)
    const two = editorWith(table)
    expect(one.getMarkdown()).toBe(table)
    typeAfter(one, 'rice', ' basmati, the long grain kind')
    typeAfter(two, 'beans', ' b0')
    const changed = (editor: Editor) =>
      editor
        .getMarkdown()
        .trimEnd()
        .split('\n')
        .filter((line, at) => line !== table.split('\n')[at])
    expect(changed(one)).toEqual(['| rice basmati, the long grain kind | 2 |'])
    expect(changed(two)).toEqual(['| beans b0 | |'])
    // a table lined up with spaces, as older versions wrote it, reads the same
    const padded = editorWith('| item  | qty |\n| ----- | :-: |\n| rice  | 2   |\n| beans |     |')
    expect(padded.getMarkdown()).toBe(table.split('\n').slice(0, 4).join('\n'))
    for (const editor of [one, two, padded]) editor.destroy()
  })

  it('a note with Windows line endings keeps them, and its form, around the line typed in', () => {
    const windows = STORED.replace(/\n/g, '\r\n')
    const editor = editorWith(windows)
    const before = editor.getMarkdown()
    typeAfter(editor, 'intro', ' x')
    expect(asWritten(windows, before, editor.getMarkdown())).toBe(
      windows.replace('intro', 'intro x'),
    )
    typeAfter(editor, 'after', ' y')
    const saved = asWritten(windows, before, editor.getMarkdown())
    expect(saved).toBe(windows.replace('intro', 'intro x').replace('after', 'after y'))
    expect(saved.replace(/\r\n/g, '')).not.toMatch(/[\r\n]/)
    // no line ending is made up after a last line that had none
    expect(asWritten('* a\r\n\r\nend', '- a\n\nend', '- a\n\nend!')).toBe('* a\r\n\r\nend!')
    editor.destroy()
  })

  it('a long note in another form on every block keeps it around the line typed in', () => {
    const blocks = Array.from(
      { length: 900 },
      (_, n) =>
        `Section ${n}\n----------\n\n__bold ${n}__ text of ${n}\n\n* item ${n}\n* other ${n}\n`,
    )
    const stored = blocks.join('\n')
    const before = stored
      .replace(/^(Section \d+)\n-+$/gm, '## $1')
      .replace(/__/g, '**')
      .replace(/^\* /gm, '- ')
    expect(stored.split('\n').length).toBeGreaterThan(6000)
    const now = before.replace('**bold 450** text of 450', '**bold 450** text of 450, typed')
    const saved = asWritten(stored, before, now)
    expect(saved).toBe(
      stored.replace('__bold 450__ text of 450', '**bold 450** text of 450, typed'),
    )
  })

  it('a note already in the editor form is saved as the editor writes it', () => {
    const editor = editorWith('# Plan\n\n- a\n- b')
    const before = editor.getMarkdown()
    typeAfter(editor, 'a', ' more')
    const now = editor.getMarkdown()
    expect(asWritten(before, before, now)).toBe(now)
    editor.destroy()
  })

  it('lines removed and added around kept ones', () => {
    expect(asWritten('a\n\n\nb\nc', 'a\n\nb\nc', 'a\n\nc\nd')).toBe('a\n\n\nc\nd')
    expect(asWritten('* a\n* b', '- a\n- b', '- a\n- b\n- c')).toBe('- a\n- b\n- c')
    expect(asWritten('* a\n\npara', '- a\n\npara', '- a\n- c\n\npara')).toBe('- a\n- c\n\npara')
    expect(asWritten('x', 'x', '')).toBe('')
    expect(asWritten('', '', 'new')).toBe('new')
  })
})
