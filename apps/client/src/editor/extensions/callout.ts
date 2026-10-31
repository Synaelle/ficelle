// SPDX-License-Identifier: AGPL-3.0-or-later
// Obsidian style callouts:
//   > [!note] Optional title
//   > Body, any blocks.

import { mergeAttributes, Node } from '@tiptap/core'

const CALLOUT = /^> ?\[!([\w-]+)\][+-]?[ \t]*([^\n]*)\n?((?:>[^\n]*(?:\n|$))*)/

export const Callout = Node.create({
  name: 'callout',
  group: 'block',
  content: 'block+',
  defining: true,

  addAttributes: () => ({
    kind: { default: 'note' },
    title: { default: '' },
  }),

  parseHTML: () => [
    {
      tag: 'div[data-callout]',
      getAttrs: (el) => ({ kind: el.dataset.callout, title: el.dataset.title ?? '' }),
    },
  ],

  renderHTML: ({ node, HTMLAttributes }) => [
    'div',
    mergeAttributes(HTMLAttributes, {
      'data-callout': node.attrs.kind,
      'data-title': node.attrs.title,
      class: 'callout',
    }),
    0,
  ],

  markdownTokenizer: {
    name: 'callout',
    level: 'block',
    start: (src: string) => /^> ?\[![\w-]+\]/m.exec(src)?.index ?? -1,
    tokenize(src, _tokens, lexer) {
      const match = CALLOUT.exec(src)
      if (!match) return undefined
      const body = (match[3] ?? '')
        .split('\n')
        .map((line) => line.replace(/^> ?/, ''))
        .join('\n')
      return {
        type: 'callout',
        raw: match[0],
        kind: (match[1] ?? 'note').toLowerCase(),
        title: (match[2] ?? '').trim(),
        tokens: lexer.blockTokens(body),
      }
    },
  },

  parseMarkdown: (token, helpers) =>
    helpers.createNode(
      'callout',
      { kind: token.kind, title: token.title },
      helpers.parseChildren(token.tokens ?? []),
    ),

  renderMarkdown(node, helpers) {
    const body = helpers.renderChildren(node.content ?? [], '\n\n').replace(/\n+$/, '')
    const head = `> [!${node.attrs?.kind}]${node.attrs?.title ? ` ${node.attrs.title}` : ''}`
    return [head, ...body.split('\n').map((line) => (line ? `> ${line}` : '>'))].join('\n')
  },
})
