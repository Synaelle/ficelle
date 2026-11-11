// SPDX-License-Identifier: AGPL-3.0-or-later
// Puts a new text in the editor as the smallest change from what it shows,
// so the caret, the selection and the scroll stay where they were: another
// device's edit lands like a paste somewhere else in the note.

import { Extension, type Editor } from '@tiptap/core'
import type { Node as PMNode, Slice } from '@tiptap/pm/model'
import { Plugin, PluginKey, type Transaction } from '@tiptap/pm/state'

/** Set on the transaction, so the editor does not save it back as an edit. */
export const REMOTE = 'remoteText'

/** A stretch of the document typed here. */
interface Typed {
  from: number
  to: number
}

/** Stretches kept: undo goes back a hundred steps. */
const TYPED_KEPT = 200

const typedKey = new PluginKey<Typed[]>('typedHere')

/**
 * Follows the text typed in this editor, so another device's text is never
 * put in the middle of it when it fits just before: undo takes back a
 * stretch typed here with all it holds.
 */
export const TypedHere = Extension.create({
  name: 'typedHere',
  addProseMirrorPlugins: () => [
    new Plugin<Typed[]>({
      key: typedKey,
      state: {
        init: () => [],
        apply(tr, typed) {
          if (!tr.docChanged) return typed
          const kept: Typed[] = []
          const add = (from: number, to: number) => {
            if (from >= to) return
            const last = kept.at(-1)
            if (last && from <= last.to && to >= last.from) {
              last.from = Math.min(last.from, from)
              last.to = Math.max(last.to, to)
            } else kept.push({ from, to })
          }
          const fresh: Typed[] = []
          if (!tr.getMeta(REMOTE))
            tr.mapping.maps.forEach((map, index) => {
              const after = tr.mapping.slice(index + 1)
              map.forEach((_oldStart, _oldEnd, start, end) =>
                fresh.push({ from: after.map(start, 1), to: after.map(end, -1) }),
              )
            })
          // text put in at either end of a stretch stays out of it
          const moved = typed.map(({ from, to }) => ({
            from: tr.mapping.map(from, 1),
            to: tr.mapping.map(to, -1),
          }))
          for (const { from, to } of [...moved, ...fresh].sort((a, b) => a.from - b.from))
            add(from, to)
          return kept.slice(-TYPED_KEPT)
        },
      },
    }),
  ],
})

/** Whether `slice` put in `doc` at `at` gives `next`. */
function reads(doc: PMNode, at: number, slice: Slice, next: PMNode) {
  try {
    return doc.replace(at, at, slice).eq(next)
  } catch {
    return false
  }
}

/** The transaction turning `doc` into `next`, null when they are the same.
 * `typed` is the text typed here, see [`TypedHere`]. */
export function patchTransaction(
  tr: Transaction,
  doc: PMNode,
  next: PMNode,
  typed: Typed[] = [],
): Transaction | null {
  let start = doc.content.findDiffStart(next.content)
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
  // text put in where the same letters follow (" bravo" before " alpha") is
  // found as late as it can be, inside the word typed here. When it reads
  // the same put in before that word, it goes there
  const at = start
  const inside = start === endA && typed.find(({ from, to }) => from < at && at < to)
  if (inside) {
    const back = start - inside.from
    const slice = next.slice(inside.from, endB - back)
    if (reads(doc, inside.from, slice, next)) {
      start = endA = inside.from
      endB -= back
    }
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
  const tr = patchTransaction(
    editor.state.tr,
    editor.state.doc,
    next,
    typedKey.getState(editor.state),
  )
  if (!tr) return false
  editor.view.dispatch(tr)
  return true
}
