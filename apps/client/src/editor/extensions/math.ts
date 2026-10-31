// SPDX-License-Identifier: AGPL-3.0-or-later
// Math kept verbatim: $inline$ and $$block$$. Without these nodes markdown
// would unescape the LaTeX (\, becomes ,). Rendering with KaTeX is optional
// and can be loaded lazily later, until then math shows as monospaced source.

import { Node } from '@tiptap/core'

// pandoc rules: no space just inside the dollars, and a closing dollar is not
// followed by a digit, so "$5 and $10" stays plain text
const INLINE = /^\$(?=\S)([^$\n]*?\S)\$(?!\d)/
const BLOCK = /^\$\$[ \t]*\n([\s\S]*?)\n\$\$[ \t]*(?:\n|$)/

export const MathInline = Node.create({
  name: 'mathInline',
  group: 'inline',
  inline: true,
  atom: true,
  addAttributes: () => ({ latex: { default: '' } }),
  parseHTML: () => [
    { tag: 'code[data-math-inline]', getAttrs: (el) => ({ latex: el.textContent ?? '' }) },
  ],
  renderHTML: ({ node }) => [
    'code',
    { 'data-math-inline': '', class: 'math' },
    `$${node.attrs.latex}$`,
  ],
  markdownTokenizer: {
    name: 'mathInline',
    level: 'inline',
    start: (src: string) => src.indexOf('$'),
    tokenize(src) {
      const match = INLINE.exec(src)
      return match ? { type: 'mathInline', raw: match[0], latex: match[1] } : undefined
    },
  },
  parseMarkdown: (token, helpers) => helpers.createNode('mathInline', { latex: token.latex }),
  renderMarkdown: (node) => `$${node.attrs?.latex ?? ''}$`,
})

export const MathBlock = Node.create({
  name: 'mathBlock',
  group: 'block',
  atom: true,
  addAttributes: () => ({ latex: { default: '' } }),
  parseHTML: () => [
    { tag: 'pre[data-math-block]', getAttrs: (el) => ({ latex: el.textContent ?? '' }) },
  ],
  renderHTML: ({ node }) => [
    'pre',
    { 'data-math-block': '', class: 'math' },
    `$$\n${node.attrs.latex}\n$$`,
  ],
  markdownTokenizer: {
    name: 'mathBlock',
    level: 'block',
    start: (src: string) => /^\$\$/m.exec(src)?.index ?? -1,
    tokenize(src) {
      const match = BLOCK.exec(src)
      return match ? { type: 'mathBlock', raw: match[0], latex: match[1] } : undefined
    },
  },
  parseMarkdown: (token, helpers) => helpers.createNode('mathBlock', { latex: token.latex }),
  renderMarkdown: (node) => `$$\n${node.attrs?.latex ?? ''}\n$$`,
})
