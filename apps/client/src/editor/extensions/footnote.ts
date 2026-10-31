// SPDX-License-Identifier: AGPL-3.0-or-later
// GitHub's footnotes: a mark in the text, `[^1]`, and the note itself on
// a line of its own, `[^1]: The text.` Kept where they are written, the
// note is not moved to the end of the page. Typed, `[^1]` inside a line
// becomes the mark at its `]`, and `[^1]: ` at a line's start makes the
// line the note.

import { InputRule, mergeAttributes, Node, textblockTypeInputRule } from '@tiptap/core'

/** A label: letters, digits, dashes and underscores, as GitHub takes. */
const LABEL = '[\\w-]+'
const REF = new RegExp(`^\\[\\^(${LABEL})\\](?!:)`)
const DEF = new RegExp(`^\\[\\^(${LABEL})\\]:[ \\t]*([^\\n]*)(?:\\n|$)`)

export const FootnoteRef = Node.create({
  name: 'footnoteRef',
  group: 'inline',
  inline: true,
  atom: true,
  addAttributes: () => ({ label: { default: '1' } }),
  parseHTML: () => [
    {
      tag: 'sup[data-footnote]',
      getAttrs: (el) => ({ label: el.dataset.footnote ?? '1' }),
    },
  ],
  renderHTML: ({ node, HTMLAttributes }) => [
    'sup',
    mergeAttributes(HTMLAttributes, { 'data-footnote': node.attrs.label, class: 'footnote-ref' }),
    `[${node.attrs.label}]`,
  ],
  markdownTokenizer: {
    name: 'footnoteRef',
    level: 'inline',
    start: (src: string) => src.indexOf('[^'),
    tokenize(src) {
      const match = REF.exec(src)
      return match ? { type: 'footnoteRef', raw: match[0], label: match[1] } : undefined
    },
  },
  parseMarkdown: (token, helpers) => helpers.createNode('footnoteRef', { label: token.label }),
  renderMarkdown: (node) => `[^${node.attrs?.label ?? '1'}]`,
  addInputRules() {
    const type = this.type
    return [
      new InputRule({
        find: new RegExp(`\\[\\^(${LABEL})\\]$`),
        handler: ({ state, range, match }) => {
          // at a line's start it may be the note itself: left as typed until
          // the `: ` that follows says so
          if (state.doc.resolve(range.from).parentOffset === 0) return null
          state.tr.replaceWith(range.from, range.to, type.create({ label: match[1] }))
        },
      }),
    ]
  },
})

export const FootnoteDef = Node.create({
  name: 'footnoteDef',
  group: 'block',
  content: 'inline*',
  defining: true,
  addAttributes: () => ({ label: { default: '1' } }),
  parseHTML: () => [
    {
      tag: 'div[data-footnote-def]',
      contentElement: '.footnote-text',
      getAttrs: (el) => ({ label: el.dataset.footnoteDef ?? '1' }),
    },
  ],
  renderHTML: ({ node, HTMLAttributes }) => [
    'div',
    mergeAttributes(HTMLAttributes, { 'data-footnote-def': node.attrs.label, class: 'footnote' }),
    ['span', { class: 'footnote-label', contenteditable: 'false' }, `${node.attrs.label}.`],
    ['span', { class: 'footnote-text' }, 0],
  ],
  markdownTokenizer: {
    name: 'footnoteDef',
    level: 'block',
    start: (src: string) => /^\[\^[\w-]+\]:/m.exec(src)?.index ?? -1,
    tokenize(src, _tokens, lexer) {
      const match = DEF.exec(src)
      if (!match) return undefined
      return {
        type: 'footnoteDef',
        raw: match[0],
        label: match[1],
        tokens: lexer.inlineTokens(match[2] ?? ''),
      }
    },
  },
  parseMarkdown: (token, helpers) =>
    helpers.createNode(
      'footnoteDef',
      { label: token.label },
      helpers.parseInline(token.tokens ?? []),
    ),
  renderMarkdown: (node, helpers) =>
    `[^${node.attrs?.label ?? '1'}]: ${helpers.renderChildren(node.content ?? [])}`,
  addInputRules() {
    return [
      textblockTypeInputRule({
        find: new RegExp(`^\\[\\^(${LABEL})\\]:\\s$`),
        type: this.type,
        getAttributes: (match) => ({ label: match[1] }),
      }),
    ]
  },
})
