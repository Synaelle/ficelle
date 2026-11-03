// SPDX-License-Identifier: AGPL-3.0-or-later
// Arrows typed the plain way become real ones: -> is the arrow chosen in
// the settings (→ by default), <- is ←, <-> is ↔, => is ⇒ and <=> is ⇔.
// Saved as the character itself. Never in code: the input rules do not run
// there, and a `code span` still being typed (an open backtick before on
// the line) is left alone too. Backspace right after brings the typed text
// back.

import { Extension, InputRule } from '@tiptap/core'

export interface TypedArrowsOptions {
  /** What -> becomes. */
  arrow: string
}

export const TypedArrows = Extension.create<TypedArrowsOptions>({
  name: 'typedArrows',
  addOptions: () => ({ arrow: '→' }),
  addInputRules() {
    // the longer ones first: at the same key, the first rule that matches
    // wins. `<-` is already ← when the > of `<->` comes
    const arrows: [RegExp, string][] = [
      [/<=>$/, '⇔'],
      [/←>$/, '↔'],
      [/->$/, this.options.arrow],
      [/<-$/, '←'],
      [/=>$/, '⇒'],
    ]
    return arrows.map(
      ([find, arrow]) =>
        new InputRule({
          find,
          handler: ({ state, range }) => {
            const $from = state.doc.resolve(range.from)
            const before = $from.parent.textBetween(0, $from.parentOffset, undefined, ' ')
            if ((before.split('`').length - 1) % 2 === 1) return null
            state.tr.insertText(arrow, range.from, range.to)
          },
        }),
    )
  },
})
