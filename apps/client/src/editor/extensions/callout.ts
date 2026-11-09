// SPDX-License-Identifier: AGPL-3.0-or-later
// Obsidian style callouts:
//   > [!note] Optional title
//   > Body, any blocks.

import { Extension, mergeAttributes, Node, type Editor } from '@tiptap/core'
import { TextSelection } from '@tiptap/pm/state'

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

/**
 * Enter on an empty line of a callout. The default lifts that line out of the
 * block, which split a callout in two, or removed one whose only line it was.
 * Here an empty last line leaves the callout, the cursor on a new line below
 * it, and any other empty line stays a new line inside. Lines with text, and
 * lists or code inside a callout, keep their own Enter.
 */
export function calloutEnter(editor: Editor): boolean {
  const { state } = editor
  const { $from, empty } = state.selection
  if (!empty) return false
  let depth = -1
  for (let d = $from.depth; d > 0; d--) {
    if ($from.node(d).type.name === 'callout') {
      depth = d
      break
    }
  }
  if (depth < 0 || $from.depth !== depth + 1) return false
  const line = $from.parent
  if (line.type.name !== 'paragraph' || line.content.size > 0) return false
  const callout = $from.node(depth)
  const last = $from.index(depth) === callout.childCount - 1
  if (!last || callout.childCount === 1) return editor.commands.splitBlock()
  return editor.commands.command(({ tr }) => {
    const start = $from.before(depth + 1)
    tr.delete(start, start + line.nodeSize)
    const below = tr.mapping.map($from.after(depth))
    // an empty line already below is used, never a second one
    const next = tr.doc.nodeAt(below)
    if (!(next?.type === line.type && next.content.size === 0)) tr.insert(below, line.type.create())
    tr.setSelection(TextSelection.create(tr.doc, below + 1))
    return true
  })
}

/** Before the default keys, which would lift the empty line out. */
export const CalloutKeys = Extension.create({
  name: 'calloutKeys',
  priority: 1000,
  addKeyboardShortcuts() {
    return { Enter: () => calloutEnter(this.editor) }
  },
})
