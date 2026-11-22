// SPDX-License-Identifier: AGPL-3.0-or-later
// Ctrl+Shift+K: the line the caret is on goes, and nothing more. One line of a code
// block, else its paragraph or heading with the quote, item or list it leaves
// empty, which would otherwise stay behind as an empty shell.

import type { Editor } from '@tiptap/core'

/** Containers a removed line takes with it once it was their only content:
 * an empty quote, bullet or list is no line worth keeping. */
const WRAPPERS = new Set([
  'blockquote',
  'callout',
  'listItem',
  'taskItem',
  'bulletList',
  'orderedList',
  'taskList',
])

/** Deletes the line the caret is on: one line of a code block, else its
 * paragraph or heading with the containers it leaves empty. */
export function deleteLine(editor: Editor): boolean {
  const at = editor.state.selection.$from
  let depth = at.depth
  while (depth > 0 && !at.node(depth).isTextblock) depth -= 1
  if (depth === 0) return false
  const block = at.node(depth)
  // a code block holds many lines: only the caret's goes
  if (block.type.spec.code && block.textContent.includes('\n')) {
    const text = block.textContent
    const offset = at.parentOffset
    let from = text.lastIndexOf('\n', offset - 1) + 1
    let to = text.indexOf('\n', offset)
    // the line's own newline goes with it, the one before it for the last line
    if (to === -1) [from, to] = [from - 1, text.length]
    else to += 1
    const start = at.start(depth)
    return editor
      .chain()
      .focus()
      .deleteRange({ from: start + from, to: start + to })
      .run()
  }
  // the line, and every container left empty by it
  while (depth > 1 && at.node(depth - 1).childCount === 1) {
    const parent = at.node(depth - 1)
    if (!WRAPPERS.has(parent.type.name)) {
      // a table cell or the like must keep a line: it is emptied instead
      return editor
        .chain()
        .focus()
        .deleteRange({ from: at.start(depth), to: at.end(depth) })
        .run()
    }
    depth -= 1
  }
  return editor
    .chain()
    .focus()
    .deleteRange({ from: at.before(depth), to: at.after(depth) })
    .run()
}
