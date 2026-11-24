// SPDX-License-Identifier: AGPL-3.0-or-later
// Find in note: every match of the find bar's words is marked in the text,
// the current one more strongly. The matches follow the text as it is
// edited while the bar is open.

import { Extension } from '@tiptap/core'
import { Plugin, PluginKey, type EditorState, type Transaction } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'
import { findInDoc, nearest, type Found } from '../findText'

export interface FindState {
  query: string
  found: Found[]
  /** The match the bar is on, -1 when there is none. */
  current: number
  marks: DecorationSet
}

export const findKey = new PluginKey<FindState>('findInNote')

/** What a transaction asks of the find: new words, or another current match. */
type FindChange = { query: string } | { current: number }

const NONE: FindState = { query: '', found: [], current: -1, marks: DecorationSet.empty }

function marked(state: EditorState, query: string, found: Found[], current: number): FindState {
  const marks = DecorationSet.create(
    state.doc,
    found.map(({ from, to }, index) =>
      Decoration.inline(from, to, {
        class: index === current ? 'find-match find-current' : 'find-match',
      }),
    ),
  )
  return { query, found, current, marks }
}

/** Sets the words to find, empty to stop finding. */
export function setFindQuery(tr: Transaction, query: string): Transaction {
  return tr.setMeta(findKey, { query } satisfies FindChange).setMeta('addToHistory', false)
}

/** Makes match `current` the current one. */
export function setFindCurrent(tr: Transaction, current: number): Transaction {
  return tr.setMeta(findKey, { current } satisfies FindChange).setMeta('addToHistory', false)
}

export const FindInNote = Extension.create({
  name: 'findInNote',

  addProseMirrorPlugins() {
    return [
      new Plugin<FindState>({
        key: findKey,
        state: {
          init: () => NONE,
          apply(tr, old, _oldState, state) {
            const change = tr.getMeta(findKey) as FindChange | undefined
            if (change && 'query' in change) {
              if (!change.query) return NONE
              // typing more of a word keeps to the match being looked at,
              // a first word starts from the caret
              const near = old.found[old.current]?.from ?? state.selection.from
              const found = findInDoc(state.doc, change.query)
              return marked(state, change.query, found, nearest(found, near))
            }
            if (change && 'current' in change)
              return marked(state, old.query, old.found, change.current)
            if (!tr.docChanged || !old.query) return old
            // an edit while the bar is open: the matches are found again,
            // the current one stays the one nearest where it was
            const found = findInDoc(state.doc, old.query)
            const was = old.found[old.current]
            return marked(
              state,
              old.query,
              found,
              nearest(found, was ? tr.mapping.map(was.from) : 0),
            )
          },
        },
        props: { decorations: (state) => findKey.getState(state)?.marks },
      }),
    ]
  },
})
