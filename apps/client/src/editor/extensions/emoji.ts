// SPDX-License-Identifier: AGPL-3.0-or-later
// GitHub's emoji shortcodes: `:smile:` becomes 😄, typed or read from a
// note, and is saved as the emoji itself, which every Markdown reader shows.
// The common ones only, GitHub's names: the full list would weigh more than
// the rest of the editor. A name not listed stays as it was written. The
// table is crates/core/src/emoji.json.

import { Extension, InputRule, Node } from '@tiptap/core'
import table from '../../../../../crates/core/src/emoji.json'

// one table for the editor and the note list's preview line (crates/core)
export const EMOJI: Record<string, string> = table

const SHORTCODE = /^:([a-z0-9_+-]+):/

/** Reads `:name:` in a note as its emoji; an unknown name stays text. */
export const EmojiShortcode = Node.create({
  name: 'emojiShortcode',
  group: 'inline',
  inline: true,
  // never in a document: the tokenizer turns a known name into plain text
  parseHTML: () => [],
  renderHTML: () => ['span', 0],
  markdownTokenizer: {
    name: 'emojiShortcode',
    level: 'inline',
    start: (src: string) => src.indexOf(':'),
    tokenize(src) {
      const match = SHORTCODE.exec(src)
      const emoji = match?.[1] ? EMOJI[match[1]] : undefined
      return match && emoji ? { type: 'emojiShortcode', raw: match[0], emoji } : undefined
    },
  },
  parseMarkdown: (token, helpers) => helpers.createTextNode(token.emoji ?? ''),
})

/** Typing `:name:` puts the emoji in its place. */
export const EmojiTyping = Extension.create({
  name: 'emojiTyping',
  addInputRules: () => [
    new InputRule({
      find: /:([a-z0-9_+-]+):$/,
      handler: ({ state, range, match }) => {
        const emoji = match[1] ? EMOJI[match[1]] : undefined
        if (!emoji) return null
        state.tr.insertText(emoji, range.from, range.to)
      },
    }),
  ],
})
