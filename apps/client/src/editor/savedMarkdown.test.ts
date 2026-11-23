// SPDX-License-Identifier: AGPL-3.0-or-later
import { Editor } from '@tiptap/core'
import type { MarkdownManager } from '@tiptap/markdown'
import { describe, expect, it } from 'vitest'
import { createExtensions } from './extensions'
import { savedMarkdown } from './savedMarkdown'

/** The writer of one kind of block, as the manager keeps it. */
interface RenderSpec {
  renderMarkdown: () => string
}

function editorWith(markdown: string) {
  return new Editor({
    element: document.createElement('div'),
    extensions: createExtensions(),
    content: markdown,
    contentType: 'markdown',
  })
}

describe('savedMarkdown', () => {
  it('is the editor markdown when it can be written', () => {
    const editor = editorWith('# Title\n\nSome *words*.\n')
    const manager = editor.markdown as MarkdownManager
    expect(savedMarkdown(manager, editor.getJSON())).toEqual({
      markdown: editor.getMarkdown(),
      error: null,
    })
    editor.destroy()
  })

  it('keeps the words of a block that cannot be written, and the other blocks as they are', () => {
    const editor = editorWith('Before **bold**\n\n> a quote\n> with two lines\n\nAfter\n')
    const manager = editor.markdown as MarkdownManager
    const registry = (manager as unknown as { nodeTypeRegistry: Map<string, RenderSpec[]> })
      .nodeTypeRegistry
    for (const spec of registry.get('blockquote') ?? [])
      spec.renderMarkdown = () => {
        throw new TypeError('a writer bug')
      }
    const { markdown, error } = savedMarkdown(manager, editor.getJSON())
    expect(error).toBeInstanceOf(TypeError)
    expect(markdown).toContain('Before **bold**')
    expect(markdown).toContain('a quote')
    expect(markdown).toContain('with two lines')
    expect(markdown).toContain('After')
    editor.destroy()
  })
})
