// SPDX-License-Identifier: AGPL-3.0-or-later
// Puts a new text in the editor as the smallest change from what it shows,
// so the caret, the selection and the scroll stay where they were: another
// device's edit lands like a paste somewhere else in the note.

import type { Editor } from '@tiptap/core'
import type { Node as PMNode } from '@tiptap/pm/model'
import type { Transaction } from '@tiptap/pm/state'

/** Set on the transaction, so the editor does not save it back as an edit. */
export const REMOTE = 'remoteText'

/** The transaction turning `doc` into `next`, null when they are the same. */
export function patchTransaction(tr: Transaction, doc: PMNode, next: PMNode): Transaction | null {
  const start = doc.content.findDiffStart(next.content)
  if (start === null) return null
  const end = doc.content.findDiffEnd(next.content)
  if (!end) return null
  let { a: endA, b: endB } = end
  // a change inside repeated text can make the two ends cross
  const overlap = start - Math.min(endA, endB)
  if (overlap > 0) {
    endA += overlap
    endB += overlap
  }
  return tr
    .replace(start, endA, next.slice(start, endB))
    .setMeta(REMOTE, true)
    .setMeta('addToHistory', false)
}

/** Shows `markdown` in the editor, changing only what differs. */
export function patchMarkdown(editor: Editor, markdown: string): boolean {
  const parsed = editor.markdown?.parse(markdown)
  if (!parsed) return false
  const next = editor.schema.nodeFromJSON(parsed)
  const tr = patchTransaction(editor.state.tr, editor.state.doc, next)
  if (!tr) return false
  editor.view.dispatch(tr)
  return true
}
