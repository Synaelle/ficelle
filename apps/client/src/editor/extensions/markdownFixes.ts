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

/**
 * Writes a table with each row as short as it can be. Upstream lines the
 * columns up with spaces, so one cell typed in rewrites every row: two
 * devices typing in two cells then each add the same spaces, and both sets
 * stay when the two are merged. With no such spaces a cell typed in changes
 * its own row and no other. The empty lines upstream adds around the table
 * go too.
 */
export function withShortRows<T extends AnyExtension>(table: T): T {
  const render = table.config.renderMarkdown as
    ((this: unknown, ...args: unknown[]) => string) | undefined
  if (!render) return table
  return table.extend({
    renderMarkdown(this: unknown, ...args: unknown[]) {
      return render
        .apply(this, args)
        .trim()
        .split('\n')
        .map((row, index) => {
          const short = row.replace(/ {2,}\|/g, ' |')
          return index === 1 ? short.replace(/-{3,}/g, '---') : short
        })
        .join('\n')
    },
  } as Partial<T['config']>) as T
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

/**
 * A link's address as markdown writes it. Upstream writes it bare, so an
 * address with a space ("My file.pdf") or a lone ")" was read back as
 * plain text or cut short. Those go between < >, where only < and > need
 * escaping.
 */
export function linkDestination(href: string): string {
  return /[\s()<>]/.test(href) ? `<${href.replace(/[<>]/g, '\\$&')}>` : href
}

/** Chunks a long note is read in, see markdownChunks. */
const CHUNK = 4096

/**
 * Long markdown cut where nothing can run on from one part into the next,
 * to be read part by part. marked cuts the rest of the text after each
 * block, which WebKit (the Linux and Mac apps) copies every time: a 300 KB
 * paste took 14 s to read whole, under a second in parts. A cut is a blank
 * line between two lines that are not indented, outside code, math and
 * folded sections, before a line that does not start a list item, a quote
 * or a table row. Notes with reference links (`[x]: url`) are read whole,
 * their definitions may be anywhere.
 */
export function markdownChunks(markdown: string, size = CHUNK): string[] {
  if (markdown.length <= size * 2 || /^ {0,3}\[[^\]\n]+\]:[ \t]/m.test(markdown)) return [markdown]
  const lines = markdown.split('\n')
  const chunks: string[] = []
  let start = 0
  let length = 0
  let fence: string | null = null
  let math = false
  let details = 0
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] ?? ''
    length += line.length + 1
    const marker = /^ {0,3}(`{3,}|~{3,})/.exec(line)?.[1]
    if (fence) {
      const close = /^ {0,3}(`{3,}|~{3,})[ \t]*$/.exec(line)?.[1]
      if (close && close[0] === fence[0] && close.length >= fence.length) fence = null
      continue
    }
    if (marker) {
      fence = marker
      continue
    }
    if (line.trim() === '$$') math = !math
    details +=
      (line.match(/<details[\s>]/g)?.length ?? 0) - (line.match(/<\/details>/g)?.length ?? 0)
    const previous = lines[i - 1] ?? ''
    const next = lines[i + 1]
    if (
      length >= size &&
      line === '' &&
      !math &&
      details <= 0 &&
      previous.trim() !== '' &&
      !/^\s/.test(previous) &&
      next !== undefined &&
      next.trim() !== '' &&
      !/^(\s|[-*+][ \t]|\d{1,9}[.)][ \t]|>|\|)/.test(next)
    ) {
      chunks.push(lines.slice(start, i).join('\n'))
      start = i + 1
      length = 0
    }
  }
  chunks.push(lines.slice(start).join('\n'))
  return chunks
}

type JSONLike = { type?: string; marks?: (string | { type: string })[] }
type LinkNode = { attrs?: { href?: string | null; title?: string | null } }
interface RenderSpec {
  renderMarkdown?: (node: LinkNode, helpers: { renderChildren(node: unknown): string }) => string
}
/** The private parts of the markdown manager these fixes replace, as of 3.32. */
interface TextEncoding {
  codeTypes: Set<string>
  parse(markdown: string): { type: 'doc'; content: unknown[] }
  nodeTypeRegistry: Map<string, RenderSpec[]>
  escapeMarkdownSyntax(text: string): string
  encodeTextForMarkdown(text: string, node: JSONLike, parent?: JSONLike): string
}

/** Markdown extension that writes text with escapeHtmlLookalikes, and link
 * addresses with linkDestination. */
export const ReadableMarkdown = Markdown.extend({
  onBeforeCreate(event) {
    this.parent?.(event)
    const manager = this.editor.markdown as unknown as TextEncoding | undefined
    if (!manager) return
    const parse = manager.parse.bind(manager)
    manager.parse = (markdown) => {
      const chunks = markdownChunks(markdown)
      if (chunks.length === 1) return parse(markdown)
      return { type: 'doc', content: chunks.flatMap((chunk) => parse(chunk).content) }
    }
    for (const spec of manager.nodeTypeRegistry.get('link') ?? []) {
      spec.renderMarkdown = (node, helpers) => {
        const href = linkDestination(node.attrs?.href ?? '')
        const title = node.attrs?.title?.replace(/"/g, '\\"')
        const text = helpers.renderChildren(node)
        return title ? `[${text}](${href} "${title}")` : `[${text}](${href})`
      }
    }
    const inCode = (node: JSONLike, parent?: JSONLike) =>
      (parent?.type !== undefined && manager.codeTypes.has(parent.type)) ||
      (node.marks ?? []).some((m) => manager.codeTypes.has(typeof m === 'string' ? m : m.type))
    manager.encodeTextForMarkdown = (text, node, parent) =>
      inCode(node, parent) ? text : escapeHtmlLookalikes(manager.escapeMarkdownSyntax(text))
  },
})
