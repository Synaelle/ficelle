// SPDX-License-Identifier: AGPL-3.0-or-later
// Enter in callouts and what a table cell may hold, through the real schema.

import { Editor, type JSONContent } from '@tiptap/core'
import { TextSelection } from '@tiptap/pm/state'
import { describe, expect, it } from 'vitest'
import { createExtensions } from './extensions'
import { calloutEnter } from './extensions/callout'

const para = (text = ''): JSONContent => ({
  type: 'paragraph',
  ...(text ? { content: [{ type: 'text', text }] } : {}),
})

function editorWith(content: JSONContent[]) {
  return new Editor({
    element: document.createElement('div'),
    extensions: createExtensions(),
    content: { type: 'doc', content },
  })
}

/** Puts the cursor in the nth paragraph of the document, in reading order. */
function cursorIn(editor: Editor, nth: number) {
  let seen = -1
  let at = -1
  editor.state.doc.descendants((node, pos) => {
    if (node.type.name === 'paragraph' && ++seen === nth) at = pos + 1
    return at < 0
  })
  editor.view.dispatch(editor.state.tr.setSelection(TextSelection.create(editor.state.doc, at)))
}

/** The blocks of the document, without the empty line the editor keeps last. */
function shape(editor: Editor) {
  const blocks = editor.getJSON().content ?? []
  const last = blocks.at(-1)
  if (last?.type === 'paragraph' && !last.content) blocks.pop()
  return blocks.map((n) => n.type)
}

describe('Enter in a callout', () => {
  it('leaves it from an empty last line, with a line below', () => {
    const editor = editorWith([
      { type: 'callout', attrs: { kind: 'note' }, content: [para('one'), para()] },
    ])
    cursorIn(editor, 1)
    expect(calloutEnter(editor)).toBe(true)
    expect(editor.getJSON().content?.map((n) => n.type)).toEqual(['callout', 'paragraph'])
    expect(editor.getJSON().content?.[0]?.content).toHaveLength(1)
    expect(editor.state.selection.$from.parent.type.name).toBe('paragraph')
    expect(editor.state.selection.$from.depth).toBe(1)
    editor.destroy()
  })

  it('keeps a callout whose only line is empty', () => {
    const editor = editorWith([{ type: 'callout', attrs: { kind: 'tip' }, content: [para()] }])
    cursorIn(editor, 0)
    expect(calloutEnter(editor)).toBe(true)
    expect(shape(editor)).toEqual(['callout'])
    expect(editor.getJSON().content?.[0]?.content).toHaveLength(2)
    editor.destroy()
  })

  it('never splits it on an empty line in the middle', () => {
    const editor = editorWith([
      { type: 'callout', attrs: { kind: 'warning' }, content: [para('a'), para(), para('b')] },
    ])
    cursorIn(editor, 1)
    expect(calloutEnter(editor)).toBe(true)
    expect(shape(editor)).toEqual(['callout'])
    expect(editor.getJSON().content?.[0]?.content).toHaveLength(4)
    editor.destroy()
  })

  it('leaves lines with text to the usual Enter', () => {
    const editor = editorWith([
      { type: 'callout', attrs: { kind: 'note' }, content: [para('text')] },
    ])
    cursorIn(editor, 0)
    expect(calloutEnter(editor)).toBe(false)
    editor.destroy()
  })
})

describe('A table cell', () => {
  it('never holds a table, one inserted there goes beside it', () => {
    const editor = new Editor({
      element: document.createElement('div'),
      extensions: createExtensions(),
      content: '| a | b |\n| - | - |\n| c | d |',
      contentType: 'markdown',
    })
    cursorIn(editor, 0)
    expect(editor.isActive('table')).toBe(true)
    editor.commands.insertTable({ rows: 2, cols: 2 })
    let nested = false
    editor.state.doc.descendants((node, pos) => {
      if (node.type.name !== 'table') return true
      const $pos = editor.state.doc.resolve(pos)
      for (let d = $pos.depth; d > 0; d--) if ($pos.node(d).type.name === 'table') nested = true
      return false
    })
    expect(nested).toBe(false)
    editor.destroy()
  })

  it('grows and shrinks by rows and columns', () => {
    const editor = new Editor({
      element: document.createElement('div'),
      extensions: createExtensions(),
      content: '| a | b |\n| - | - |\n| c | d |',
      contentType: 'markdown',
    })
    cursorIn(editor, 2)
    editor.commands.addRowAfter()
    editor.commands.addColumnAfter()
    expect(
      editor
        .getMarkdown()
        .split('\n')
        .filter((l) => l.startsWith('|')),
    ).toHaveLength(4)
    const header = editor
      .getMarkdown()
      .split('\n')
      .find((l) => l.startsWith('|'))
    expect(header?.split('|').length).toBe(5)
    editor.commands.deleteRow()
    editor.commands.deleteColumn()
    expect(
      editor
        .getMarkdown()
        .split('\n')
        .filter((l) => l.startsWith('|')),
    ).toHaveLength(3)
    editor.destroy()
  })
})
