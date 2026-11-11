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

/** What differs between two documents: `doc` from `start` to `endA` is
 * `next` from `start` to `endB`. */
interface Change {
  start: number
  endA: number
  endB: number
}

/** Where `doc` and `next` differ, null when they are the same. `typed` is
 * the text typed here, see [`TypedHere`]. */
function changed(doc: PMNode, next: PMNode, typed: Typed[] = []): Change | null {
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
  return { start, endA, endB }
}

const remote = (tr: Transaction) => tr.setMeta(REMOTE, true).setMeta('addToHistory', false)

/** The transaction turning `doc` into `next`, null when they are the same.
 * `typed` is the text typed here, see [`TypedHere`]. */
export function patchTransaction(
  tr: Transaction,
  doc: PMNode,
  next: PMNode,
  typed: Typed[] = [],
): Transaction | null {
  const change = changed(doc, next, typed)
  if (!change) return null
  return remote(tr.replace(change.start, change.endA, next.slice(change.start, change.endB)))
}

const read = (editor: Editor, markdown: string) => {
  const parsed = editor.markdown?.parse(markdown)
  return parsed ? editor.schema.nodeFromJSON(parsed) : null
}

/**
 * The editor holds what its Markdown cannot: a space just typed at the end
 * of a line, an empty line just made. Changing the document into `next`
 * would take those away under the caret and send it elsewhere. This is the
 * change from the note as its Markdown reads to `next`, made beside them.
 * `false` when there is nothing to change, null when the other device
 * changed that very place.
 */
function besideTyping(editor: Editor, next: PMNode): Transaction | false | null {
  const doc = editor.state.doc
  const saved = read(editor, editor.getMarkdown())
  if (!saved) return null
  const theirs = changed(saved, next)
  if (!theirs) return false
  const mine = changed(doc, saved)
  if (!mine) return null
  const slice = next.slice(theirs.start, theirs.endB)
  if (theirs.endA <= mine.start)
    return remote(editor.state.tr.replace(theirs.start, theirs.endA, slice))
  if (theirs.start < mine.endB) return null
  const moved = mine.endA - mine.endB
  return remote(editor.state.tr.replace(theirs.start + moved, theirs.endA + moved, slice))
}

/** Shows `markdown` in the editor, changing only what differs. */
export function patchMarkdown(editor: Editor, markdown: string): boolean {
  const next = read(editor, markdown)
  if (!next) return false
  const { doc, selection } = editor.state
  const typed = typedKey.getState(editor.state)
  const change = changed(doc, next, typed)
  if (!change) return false
  // the caret is in what would be replaced: not before a look at whether
  // that is only text the Markdown does not hold
  const atCaret = selection.to >= change.start && selection.from <= change.endA
  const beside = atCaret ? besideTyping(editor, next) : null
  if (beside === false) return false
  const tr = beside ?? patchTransaction(editor.state.tr, doc, next, typed)
  if (!tr) return false
  editor.view.dispatch(tr)
  return true
}
