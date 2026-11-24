// SPDX-License-Identifier: AGPL-3.0-or-later
// Where a word typed in the find bar is in a note, case and accents aside as
// in the note search: in plain text (the Raw view) and in the formatted
// document, where a word may run across bold or a link inside a paragraph.

import type { Node as PMNode } from '@tiptap/pm/model'

/** A piece of text found, from its first character to just after its last. */
export interface Found {
  from: number
  to: number
}

/** `text` lowercased and without accents, with where each of its characters
 * starts in `text` and, last, the length of `text`. */
function folded(text: string): { plain: string; starts: number[] } {
  let plain = ''
  const starts: number[] = []
  let at = 0
  for (const char of text) {
    // most notes are mostly plain letters, normalizing each one costs
    const one =
      char < '\u0080'
        ? char.toLowerCase()
        : char.toLowerCase().normalize('NFD').replace(/\p{M}/gu, '')
    for (let i = 0; i < one.length; i++) starts.push(at)
    plain += one
    at += char.length
  }
  starts.push(text.length)
  return { plain, starts }
}

const foldedQuery = (query: string) => folded(query).plain

/** Every place `query` is in `text`, one after another without overlap.
 * An accent left after a found letter (é written as e and its mark) is
 * part of what was found. */
export function findAll(text: string, query: string): Found[] {
  const wanted = foldedQuery(query)
  if (!wanted) return []
  const { plain, starts } = folded(text)
  const found: Found[] = []
  let at = plain.indexOf(wanted)
  while (at !== -1) {
    const from = starts[at] ?? 0
    // a letter that folded into more than one is found whole
    let end = at + wanted.length
    while (end < plain.length && starts[end] === starts[end - 1]) end++
    found.push({ from, to: starts[end] ?? text.length })
    at = plain.indexOf(wanted, at + wanted.length)
  }
  return found
}

/** The first match ending after `at`, else the first of all; -1 for none. */
export const nearest = (found: Found[], at: number) =>
  found.length === 0
    ? -1
    : Math.max(
        0,
        found.findIndex((f) => f.to > at),
      )

/** Stands for what is not text in a paragraph (a picture, a line break, a
 * link to a note): nothing typed in the find bar matches across it. */
const BREAK = '\n'

/** Every place `query` is in the text of `doc`, as document positions. A
 * match stays within one paragraph, heading, list item line or cell. */
export function findInDoc(doc: PMNode, query: string): Found[] {
  if (!foldedQuery(query)) return []
  const found: Found[] = []
  doc.descendants((block, blockPos) => {
    if (!block.isTextblock) return true
    let text = ''
    const positions: number[] = []
    block.forEach((child, offset) => {
      const pos = blockPos + 1 + offset
      if (child.isText && child.text) {
        for (let i = 0; i < child.text.length; i++) positions.push(pos + i)
        text += child.text
      } else {
        positions.push(pos)
        text += BREAK
      }
    })
    for (const { from, to } of findAll(text, query)) {
      const start = positions[from]
      const end = positions[to - 1]
      if (start !== undefined && end !== undefined) found.push({ from: start, to: end + 1 })
    }
    return false
  })
  return found
}
