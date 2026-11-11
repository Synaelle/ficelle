// SPDX-License-Identifier: AGPL-3.0-or-later
// Puts a new text in the editor as the smallest change from what it shows,
// so the caret, the selection and the scroll stay where they were: another
// device's edit lands like a paste somewhere else in the note.

import { Extension, type Editor } from '@tiptap/core'
import type { Fragment, Node as PMNode } from '@tiptap/pm/model'
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

/** One change: `doc` from `start` to `endA` becomes `next` from `startB`
 * to `endB`. */
interface Change {
  start: number
  endA: number
  startB: number
  endB: number
}

/** Blocks lined up in a table of at most this many cells, else the whole
 * stretch that differs is one change. */
const TOO_MANY = 250_000

/** Two texts that are mostly one another: what they start and end with
 * alike is at least half of the shorter. */
function alike(a: string, b: string) {
  const least = Math.min(a.length, b.length)
  let head = 0
  while (head < least && a[head] === b[head]) head++
  let tail = 0
  while (tail < least - head && a[a.length - 1 - tail] === b[b.length - 1 - tail]) tail++
  return (head + tail) * 2 >= least
}

/**
 * The children of `a` and `b` that go together, in order, as pairs of
 * indexes, between the first and last that differ: the same block, or one
 * of the same kind whose text is mostly the same (a paragraph another
 * device typed in, a row with a cell changed).
 */
function sameBlocks(a: Fragment, b: Fragment, head: number, tail: number): [number, number][] {
  const rows = a.childCount - head - tail
  const columns = b.childCount - head - tail
  if (rows === 0 || columns === 0 || rows * columns > TOO_MANY) return []
  const text = (from: Fragment, count: number) =>
    Array.from({ length: count }, (_, at) => from.child(head + at).textContent)
  const inA = text(a, rows)
  const inB = text(b, columns)
  const together = (r: number, c: number) => {
    const [one, other] = [a.child(head + r), b.child(head + c)]
    return one.eq(other) || (one.sameMarkup(other) && alike(inA[r] as string, inB[c] as string))
  }
  const width = columns + 1
  const run = new Uint16Array((rows + 1) * width)
  for (let r = rows - 1; r >= 0; r--)
    for (let c = columns - 1; c >= 0; c--)
      run[r * width + c] = together(r, c)
        ? (run[(r + 1) * width + c + 1] as number) + 1
        : Math.max(run[(r + 1) * width + c] as number, run[r * width + c + 1] as number)
  const pairs: [number, number][] = []
  let r = 0
  let c = 0
  while (r < rows && c < columns) {
    if (together(r, c)) pairs.push([head + r++, head + c++])
    else if ((run[(r + 1) * width + c] as number) >= (run[r * width + c + 1] as number)) r++
    else c++
  }
  return pairs
}

/** The one change between two runs of text and marks, `a` at `at` in the
 * document and `b` at `atB` in the next one. `typed` is the text typed
 * here, see [`TypedHere`]. */
function textChange(a: PMNode, b: PMNode, at: number, atB: number, typed: Typed[]): Change[] {
  let start = a.content.findDiffStart(b.content)
  if (start === null) return []
  const end = a.content.findDiffEnd(b.content)
  if (!end) return []
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
  const found = at + start
  const inside = start === endA && typed.find(({ from, to }) => from < found && found < to)
  if (inside && inside.from >= at) {
    const back = found - inside.from
    const put = b.slice(start - back, endB - back)
    let reads = false
    try {
      reads = a.replace(start - back, start - back, put).eq(b)
    } catch {
      // not a place this text can go
    }
    if (reads) {
      start -= back
      endA = start
      endB -= back
    }
  }
  return [{ start: at + start, endA: at + endA, startB: atB + start, endB: atB + endB }]
}

/**
 * What differs between `a` and `b`, block by block, so text that changed
 * above the caret and text that changed under it are two changes, with the
 * caret left alone between them. `at` and `atB` are where the content of
 * each starts.
 */
function changes(a: PMNode, b: PMNode, at: number, atB: number, typed: Typed[]): Change[] {
  if (a.inlineContent || b.inlineContent)
    return a.inlineContent && b.inlineContent
      ? textChange(a, b, at, atB, typed)
      : [{ start: at, endA: at + a.content.size, startB: atB, endB: atB + b.content.size }]
  const least = Math.min(a.childCount, b.childCount)
  let head = 0
  while (head < least && a.child(head).eq(b.child(head))) head++
  let tail = 0
  while (
    tail < least - head &&
    a.child(a.childCount - 1 - tail).eq(b.child(b.childCount - 1 - tail))
  )
    tail++
  const pairs = sameBlocks(a.content, b.content, head, tail)
  pairs.push([a.childCount - tail, b.childCount - tail])
  // where each child starts
  const starts = (node: PMNode, from: number) => {
    const list = [from]
    node.forEach((child) => list.push((list.at(-1) as number) + child.nodeSize))
    return list
  }
  const inA = starts(a, at)
  const inB = starts(b, atB)
  const out: Change[] = []
  let i = head
  let j = head
  /** Child `x` becomes child `y`: what changed inside, when they are of a kind. */
  const inside = (x: number, y: number) => {
    const [was, now] = [a.child(x), b.child(y)]
    if (was.eq(now)) return
    if (!was.isLeaf && was.sameMarkup(now))
      out.push(...changes(was, now, (inA[x] as number) + 1, (inB[y] as number) + 1, typed))
    else
      out.push({
        start: inA[x] as number,
        endA: inA[x + 1] as number,
        startB: inB[y] as number,
        endB: inB[y + 1] as number,
      })
  }
  for (const [nextI, nextJ] of pairs) {
    // the blocks with nothing to go with: taken one for one, in order (a
    // table that grew is still that table), the rest taken out or put in
    const both = Math.min(nextI - i, nextJ - j)
    for (let k = 0; k < both; k++) inside(i + k, j + k)
    if (nextI - i !== nextJ - j)
      out.push({
        start: inA[i + both] as number,
        endA: inA[nextI] as number,
        startB: inB[j + both] as number,
        endB: inB[nextJ] as number,
      })
    if (nextI < a.childCount - tail) inside(nextI, nextJ)
    i = nextI + 1
    j = nextJ + 1
  }
  return out
}

/** The transaction turning `doc` into `next`, null when they are the same.
 * `typed` is the text typed here, see [`TypedHere`]. */
export function patchTransaction(
  tr: Transaction,
  doc: PMNode,
  next: PMNode,
  typed: Typed[] = [],
): Transaction | null {
  const list = changes(doc, next, 0, 0, typed)
  if (list.length === 0) return null
  // from the last one back, so the places of the earlier ones hold
  for (const { start, endA, startB, endB } of list.reverse())
    tr.replace(start, endA, next.slice(startB, endB))
  return tr.setMeta(REMOTE, true).setMeta('addToHistory', false)
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
