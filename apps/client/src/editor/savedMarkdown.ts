// SPDX-License-Identifier: AGPL-3.0-or-later
// The Markdown a note is saved as. Writing it can fail on a document the
// writer does not expect (a bug, here or upstream): the whole note is then
// written block by block, and a block that still fails goes in as its
// plain text, so such a bug costs that block its formatting and never the
// words typed in the note.

import type { JSONContent } from '@tiptap/core'
import type { MarkdownManager } from '@tiptap/markdown'

/** The words of `node` and all it holds, a line between two blocks. */
function plainText(node: JSONContent): string {
  if (node.text !== undefined) return node.text
  if (node.type === 'hardBreak') return '\n'
  const parts = node.content ?? []
  const inline = parts.some((part) => part.text !== undefined || part.type === 'hardBreak')
  return parts.map(plainText).join(inline ? '' : '\n\n')
}

/** `doc` as Markdown, and the error writing it whole threw, or null. */
export function savedMarkdown(
  manager: MarkdownManager,
  doc: JSONContent,
): { markdown: string; error: unknown } {
  try {
    return { markdown: manager.serialize(doc), error: null }
  } catch (error) {
    const blocks = (doc.content ?? []).map((block) => {
      try {
        return manager.serialize({ type: 'doc', content: [block] })
      } catch {
        const lines = plainText(block).split('\n\n')
        const content = lines.map((line) => ({
          type: 'paragraph',
          ...(line ? { content: [{ type: 'text', text: line }] } : {}),
        }))
        return manager.serialize({ type: 'doc', content })
      }
    })
    return { markdown: blocks.join('\n\n'), error }
  }
}
