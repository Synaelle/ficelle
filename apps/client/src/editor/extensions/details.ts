// SPDX-License-Identifier: AGPL-3.0-or-later
// GitHub's folded sections, written in HTML:
//   <details>
//   <summary>The title</summary>
//
//   Any blocks.
//
//   </details>
// In the editor the section stays open, its title a line of its own above
// the blocks: folding a part of the note while writing it would hide it.

import { mergeAttributes, Node } from '@tiptap/core'

const DETAILS =
  /^<details(?: open)?>[ \t]*\n?(?:[ \t]*<summary>([\s\S]*?)<\/summary>[ \t]*\n)?([\s\S]*?)\n?<\/details>[ \t]*(?:\n|$)/

export const DetailsSummary = Node.create({
  name: 'detailsSummary',
  content: 'inline*',
  defining: true,
  parseHTML: () => [{ tag: 'summary' }, { tag: 'div[data-details-summary]' }],
  renderHTML: ({ HTMLAttributes }) => [
    'div',
    mergeAttributes(HTMLAttributes, { 'data-details-summary': '', class: 'details-summary' }),
    0,
  ],
  renderMarkdown: (node, helpers) => helpers.renderChildren(node.content ?? []),
})

export const Details = Node.create({
  name: 'details',
  group: 'block',
  content: 'detailsSummary block+',
  defining: true,
  parseHTML: () => [{ tag: 'details' }, { tag: 'div[data-details]' }],
  renderHTML: ({ HTMLAttributes }) => [
    'div',
    mergeAttributes(HTMLAttributes, { 'data-details': '', class: 'details' }),
    0,
  ],
  markdownTokenizer: {
    name: 'details',
    level: 'block',
    start: (src: string) => /^<details/m.exec(src)?.index ?? -1,
    tokenize(src, _tokens, lexer) {
      const match = DETAILS.exec(src)
      if (!match) return undefined
      return {
        type: 'details',
        raw: match[0],
        summary: lexer.inlineTokens((match[1] ?? '').trim()),
        tokens: lexer.blockTokens((match[2] ?? '').trim()),
      }
    },
  },
  parseMarkdown: (token, helpers) => {
    const blocks = helpers.parseChildren(token.tokens ?? [])
    return helpers.createNode('details', {}, [
      helpers.createNode('detailsSummary', {}, helpers.parseInline(token.summary ?? [])),
      // a section needs a block, an empty one gets an empty paragraph
      ...(blocks.length ? blocks : [helpers.createNode('paragraph')]),
    ])
  },
  renderMarkdown: (node, helpers) => {
    const [summary, ...blocks] = node.content ?? []
    const title = summary ? helpers.renderChildren(summary) : ''
    const body = helpers.renderChildren(blocks, '\n\n').replace(/\n+$/, '')
    return `<details>\n<summary>${title}</summary>\n\n${body}\n\n</details>`
  },
})
