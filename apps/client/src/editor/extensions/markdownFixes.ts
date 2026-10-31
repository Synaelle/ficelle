// SPDX-License-Identifier: AGPL-3.0-or-later
// Fixes for @tiptap/markdown 3.32, found by the editor evaluation. Keep until
// upstream ships them, the round trip test in markdown.test.ts fails if a fix
// is lost or broken.

import type { AnyExtension } from '@tiptap/core'
import Paragraph from '@tiptap/extension-paragraph'
import { Markdown } from '@tiptap/markdown'

/**
 * Paragraph that keeps line start escapes and image only paragraphs.
 *
 * Upstream only escapes inline syntax, so a paragraph starting with "# ",
 * "1. ", "- " or "> " was written unescaped and came back as a heading, list
 * or quote. Upstream also lifts an image alone in a paragraph out as a block,
 * which is invalid once images are inline (needed to float them in text).
 */
export const SafeParagraph = Paragraph.extend({
  parseMarkdown: (token, helpers) => {
    // a line holding only &nbsp; is how upstream (and 0.1.0) kept an empty
    // line: it comes back empty, not as text the person sees
    // (not trim(): it removes \u00a0 too)
    if (/^[ \t]*(&nbsp;|\u00a0)[ \t]*\n*$/.test(token.raw ?? ''))
      return helpers.createNode('paragraph', undefined, [])
    return helpers.createNode('paragraph', undefined, helpers.parseInline(token.tokens ?? []))
  },
  renderMarkdown(node, helpers, context) {
    // An empty line is written as nothing. Upstream writes &nbsp; to keep
    // several empty lines in a row, which other apps show as text; plain
    // markdown has no way to keep them, like every other markdown editor.
    if (!node.content?.length) return ''
    const out: string = this.parent?.(node, helpers, context) ?? ''
    return out
      .replace(
        /^( {0,3})(#{1,6}(?=\s|$)|[-+*](?=\s)|>)/gm,
        (_match, space: string, marker: string) => `${space}\\${marker}`,
      )
      .replace(/^( {0,3}\d{1,9})([.)])(?=\s)/gm, '$1\\$2')
  },
})

/**
 * marked calls every block tokenizer's start(src) with the whole rest of the
 * note before each paragraph, so a start() that scans it makes parsing
 * quadratic (Table, TaskList and Callout do). A paragraph never runs past a
 * blank line, so scanning up to the next blank line finds the same start.
 */
function boundedStart(start: (src: string) => number) {
  return (src: string) => {
    const end = src.indexOf('\n\n')
    return start(end === -1 ? src : src.slice(0, end + 1))
  }
}

/**
 * Applies boundedStart to an extension's tokenizer. `guard` must match the
 * start of the source before the tokenizer runs at all. `extent` gives the
 * part of the source the tokenizer can use, for tokenizers that do work on
 * all they are given (TaskList splits it into lines).
 */
export function withBoundedStart<T extends AnyExtension>(
  extension: T,
  guard?: RegExp,
  extent?: (src: string) => string,
): T {
  const tokenizer = extension.config.markdownTokenizer
  if (!tokenizer) return extension
  const { start, tokenize } = tokenizer
  return extension.extend({
    markdownTokenizer: {
      ...tokenizer,
      start: typeof start === 'function' ? boundedStart(start) : start,
      tokenize: guard
        ? function (this: unknown, src: string, ...rest: unknown[]) {
            return guard.test(src)
              ? (tokenize as (...args: unknown[]) => unknown).call(
                  this,
                  extent ? extent(src) : src,
                  ...rest,
                )
              : undefined
          }
        : tokenize,
    },
  }) as T
}

/** A task item line: "- [ ] text" or "* [x] text". */
export const TASK_GUARD = /^ {0,3}[-+*][ \t]+\[[ xX]\][ \t]/

const TASK_LINE = /^\s*[-+*]\s+\[[ xX]\]\s+/

/**
 * The lines a task list can take: up to the first line that is not
 * indented and is either not a task or comes after a blank line.
 * TaskList's tokenizer (through parseIndentedBlocks) never goes past it,
 * but splits everything it is given into lines first, which made a note
 * with many task lists quadratic (a 50,000 word note took 0.7 s to read
 * instead of 40 ms).
 */
export function taskListExtent(src: string): string {
  let at = 0
  let afterBlank = false
  while (at < src.length) {
    const end = src.indexOf('\n', at)
    const line = src.slice(at, end === -1 ? src.length : end)
    const blank = line.trim() === ''
    if (!blank && !/^\s/.test(line) && (afterBlank || !TASK_LINE.test(line))) {
      return src.slice(0, at)
    }
    afterBlank = blank
    if (end === -1) break
    at = end + 1
  }
  return src
}

/**
 * Escapes only the & and < that markdown would read as an entity or a tag.
 * Upstream writes every &, < and > as an html entity, so "a -> b" was saved
 * as "a -&gt; b" and showed that way in previews and other apps. A > at line
 * start is already escaped by SafeParagraph.
 */
export function escapeHtmlLookalikes(text: string): string {
  return text.replace(/&(?=#?\w+;)/g, '\\&').replace(/<(?=[A-Za-z/!?])/g, '\\<')
}

type JSONLike = { type?: string; marks?: (string | { type: string })[] }
/** The private parts of the markdown manager this fix replaces, as of 3.32. */
interface TextEncoding {
  codeTypes: Set<string>
  escapeMarkdownSyntax(text: string): string
  encodeTextForMarkdown(text: string, node: JSONLike, parent?: JSONLike): string
}

/** Markdown extension that writes text with escapeHtmlLookalikes. */
export const ReadableMarkdown = Markdown.extend({
  onBeforeCreate(event) {
    this.parent?.(event)
    const manager = this.editor.markdown as unknown as TextEncoding | undefined
    if (!manager) return
    const inCode = (node: JSONLike, parent?: JSONLike) =>
      (parent?.type !== undefined && manager.codeTypes.has(parent.type)) ||
      (node.marks ?? []).some((m) => manager.codeTypes.has(typeof m === 'string' ? m : m.type))
    manager.encodeTextForMarkdown = (text, node, parent) =>
      inCode(node, parent) ? text : escapeHtmlLookalikes(manager.escapeMarkdownSyntax(text))
  },
})
