// SPDX-License-Identifier: AGPL-3.0-or-later
// Fixes for @tiptap/markdown 3.32, found by the editor evaluation. Keep until
// upstream ships them, the round trip test in markdown.test.ts fails if a fix
// is lost or broken.

import type { AnyExtension, JSONContent } from '@tiptap/core'
import Paragraph from '@tiptap/extension-paragraph'
import { Markdown } from '@tiptap/markdown'

/**
 * Paragraph that keeps line start escapes and image only paragraphs.
 *
 * Upstream only escapes inline syntax, so a paragraph starting with "# ",
 * "1. ", "- ", "> " or "---" was written unescaped and came back as a
 * heading, list, quote or horizontal line. Upstream also lifts an image
 * alone in a paragraph out as a block, which is invalid once images are
 * inline (needed to float them in text).
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
    // dashes typed as text alone on a line would come back as a horizontal line
    return out
      .replace(
        /^( {0,3})(#{1,6}(?=\s|$)|[-+*](?=\s)|>)/gm,
        (_match, space: string, marker: string) => `${space}\\${marker}`,
      )
      .replace(/^( {0,3}\d{1,9})([.)])(?=\s)/gm, '$1\\$2')
      .replace(/^( {0,3})(?=(?:-[ \t]*){3,}$)/gm, '$1\\')
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

/** The cells of a table row as written, split at the pipes that are not
 * escaped or in code, as the reader splits them. */
function rowCells(line: string): string[] {
  const masked = line.replace(/\\.|(`+)[\s\S]*?\1/g, (part) => 'x'.repeat(part.length))
  const cells: string[] = []
  let from = 0
  for (let at = 0; at <= line.length; at++)
    if (at === line.length || masked[at] === '|') {
      cells.push(line.slice(from, at))
      from = at + 1
    }
  // a pipe at either end opens or closes the row
  if (cells.length > 1 && cells[0]?.trim() === '') cells.shift()
  if (cells.length > 1 && cells.at(-1)?.trim() === '') cells.pop()
  return cells.map((cell) => cell.trim().replace(/\\\|/g, '|'))
}

type TableToken = { raw?: string; header?: unknown[]; rows?: unknown[] }
type ParseHelpers = {
  tokenizeInline(src: string): unknown[]
  parseInline(tokens: unknown[]): JSONContent[]
}

/**
 * Reads a table whose rows have more cells than its header with every
 * cell. Upstream drops the cells past the header's, so they left the note
 * at its next save: they are read from the row as written, and the header
 * and the shorter rows get empty cells up to the longest row.
 */
export function withAllCells<T extends AnyExtension>(table: T): T {
  const parse = table.config.parseMarkdown as
    ((this: unknown, token: TableToken, helpers: ParseHelpers) => JSONContent) | undefined
  if (!parse) return table
  return table.extend({
    parseMarkdown(this: unknown, token: TableToken, helpers: ParseHelpers) {
      const parsed = parse.call(this, token, helpers)
      const rows = parsed.content ?? []
      const lines = (token.raw ?? '').replace(/\n+$/, '').split('\n').slice(2)
      const width = token.header?.length ?? 0
      if (lines.length !== token.rows?.length || rows.length !== lines.length + 1) return parsed
      const cell = (type: string, content: JSONContent[] = []) => ({
        type,
        content: [{ type: 'paragraph', content }],
      })
      lines.forEach((line, index) => {
        const extra = rowCells(line).slice(width)
        rows[index + 1]?.content?.push(
          ...extra.map((text) =>
            cell('tableCell', helpers.parseInline(helpers.tokenizeInline(text))),
          ),
        )
      })
      const most = rows.reduce((longest, row) => Math.max(longest, row.content?.length ?? 0), 0)
      rows.forEach((row, index) => {
        while ((row.content?.length ?? most) < most)
          row.content?.push(cell(index === 0 ? 'tableHeader' : 'tableCell'))
      })
      return parsed
    },
  } as Partial<T['config']>) as T
}

/** A numbered item with nothing after its number: "2." alone on its line. */
const EMPTY_NUMBER = /^([ \t]*\d{1,9}[.)])$/gm

/** Lines the ordered list tokenizer is first given, doubled while the list
 * fills them, as upstream reads them. */
const LIST_LINES = 64

/**
 * Reads a numbered item with no text after its number. Upstream's ordered
 * list wants a space after the number, so "2." alone was read as text of
 * the item above it and the next items were numbered one less, and a note
 * that was only "1." made an item with no line at all, which threw at
 * every save after it. A space is put after such numbers before the list
 * is read, and each item gets its empty line.
 */
export function withEmptyItems<T extends AnyExtension>(list: T): T {
  const tokenizer = list.config.markdownTokenizer
  const parse = list.config.parseMarkdown as
    ((this: unknown, ...args: unknown[]) => JSONContent | JSONContent[]) | undefined
  if (!tokenizer || !parse) return list
  const tokenize = tokenizer.tokenize as (
    this: unknown,
    src: string,
    ...rest: unknown[]
  ) => { raw: string } | undefined
  return list.extend({
    markdownTokenizer: {
      ...tokenizer,
      tokenize(this: unknown, src: string, ...rest: unknown[]) {
        // lists of letters (a. b. c.) are left to upstream as they are
        const indent = /^([ \t]*)\d{1,9}[.)](?:\s|$)/.exec(src)?.[1]?.length
        if (indent === undefined) return tokenize.call(this, src, ...rest)
        // an item set less far in than the first is dropped by upstream: it
        // starts a list of its own
        const outer =
          indent > 0 ? new RegExp(`\\n[ \\t]{0,${indent - 1}}\\d{1,9}[.)](?:\\s|$)`) : null
        for (let count = LIST_LINES; ; count *= 2) {
          let end = -1
          for (let i = 0; i < count && (end = src.indexOf('\n', end + 1)) !== -1; i++);
          let head = end === -1 ? src : src.slice(0, end)
          const cut = outer ? head.search(outer) : -1
          if (cut !== -1) head = head.slice(0, cut)
          const token = tokenize.call(this, head.replace(EMPTY_NUMBER, '$1 '), ...rest)
          if (!token) return token
          const lines = token.raw.split('\n').length
          if (lines < count || end === -1 || cut !== -1) {
            // the source as written, not with the spaces put in
            token.raw = src.split('\n', lines).join('\n')
            return token
          }
        }
      },
    },
    parseMarkdown(this: unknown, ...args: unknown[]) {
      const parsed = parse.apply(this, args)
      for (const list of Array.isArray(parsed) ? parsed : [parsed])
        for (const item of list.content ?? [])
          if (item.content?.[0]?.type !== 'paragraph')
            item.content = [{ type: 'paragraph', content: [] }, ...(item.content ?? [])]
      return parsed
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

/**
 * Whether a piece of text is a link showing its own web address and nothing
 * else, as one typed out is: written `<address>`, which every Markdown reader
 * takes for that same link whatever stands around it.
 */
export function showsItsAddress(node: JSONLike): boolean {
  const [mark, ...others] = node.marks ?? []
  if (typeof mark !== 'object' || mark.type !== 'link' || others.length > 0) return false
  const href = mark.attrs?.href
  return !mark.attrs?.title && node.text === href && /^https?:\/\/[^\s<>]+$/i.test(href ?? '')
}

/** Chunks a long note is read in, see markdownChunks. */
const CHUNK = 4096

/**
 * Long markdown cut where nothing can run on from one part into the next,
 * to be read part by part. marked cuts the rest of the text after each
 * block, which WebKit (the Linux app) copies every time: a 300 KB
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

type MarkLike = { type: string; attrs?: { href?: string | null; title?: string | null } }
type JSONLike = { type?: string; text?: string; marks?: (string | MarkLike)[] }
type LinkNode = { attrs?: { href?: string | null; title?: string | null } }
interface RenderSpec {
  renderMarkdown?: (node: LinkNode, helpers: { renderChildren(node: unknown): string }) => string
}
type LinkToken = { href?: string; tokens?: { type: string; raw: string; text: string }[] }
interface ParseSpec {
  parseMarkdown?: (token: LinkToken, helpers: unknown) => unknown
}
/** The letters and digits of `text`, in order. */
const lettersOf = (text: string) => (text.match(/[\p{L}\p{N}]/gu) ?? []).join('')

/** The text `content` holds, at any depth. */
function textOf(content: JSONContent | JSONContent[] | null): string {
  if (!content) return ''
  if (Array.isArray(content)) return content.map(textOf).join('')
  return (content.text ?? '') + textOf(content.content ?? [])
}

/**
 * Whether HTML read as `parsed` kept the words of `html`: all its letters
 * and digits outside its tags and entities, in order. A comment, a script
 * or a doctype comes out of the reader empty.
 */
export function keepsWords(html: string, parsed: JSONContent | JSONContent[] | null): boolean {
  const said = lettersOf(html.replace(/<\/?[A-Za-z][^<>]*>|&#?\w+;/g, ''))
  const kept = lettersOf(textOf(parsed))
  let at = 0
  for (const letter of kept) if (letter === said[at]) at++
  return at >= said.length
}

/** The private parts of the markdown manager these fixes replace, as of 3.32. */
interface TextEncoding {
  codeTypes: Set<string>
  parseHTMLToken(token: { text?: string; raw?: string; block?: boolean }): JSONContent | null
  htmlAsLiteralText(html: string, isBlock: boolean): JSONContent | null
  parseFallbackToken(token: { type?: string; raw?: string }, ...rest: unknown[]): unknown
  parse(markdown: string): { type: 'doc'; content: JSONContent[] }
  nodeTypeRegistry: Map<string, RenderSpec[]>
  registry: Map<string, ParseSpec[]>
  escapeMarkdownSyntax(text: string): string
  encodeTextForMarkdown(text: string, node: JSONLike, parent?: JSONLike): string
  renderNodesWithMarkBoundaries(nodes: JSONLike[], ...rest: unknown[]): string
}

/** Markdown extension that writes text with escapeHtmlLookalikes, and link
 * addresses with linkDestination, or as `<address>` (showsItsAddress). */
export const ReadableMarkdown = Markdown.extend({
  onBeforeCreate(event) {
    // upstream reads the note's markdown while it sets itself up, before the
    // reading in parts below exists: held back here, so a long note opens as
    // fast as it pastes
    const options = this.editor.options
    const initial = options.contentType === 'markdown' && typeof options.content === 'string'
    if (initial) options.contentType = undefined
    this.parent?.(event)
    if (initial) options.contentType = 'markdown'
    const manager = this.editor.markdown as unknown as TextEncoding | undefined
    if (!manager) return
    const parse = manager.parse.bind(manager)
    manager.parse = (markdown) => {
      // the reader drops the \r of Windows line endings anyway: without it,
      // the blank lines between blocks show and the note is cut there
      const chunks = markdownChunks(
        markdown.includes('\r') ? markdown.replace(/\r\n?/g, '\n') : markdown,
      )
      if (chunks.length === 1) return parse(markdown)
      return { type: 'doc', content: chunks.flatMap((chunk) => parse(chunk).content) }
    }
    // HTML the editor cannot show (a comment, a script) stays as its text
    // rather than leaving the note
    const parseHTML = manager.parseHTMLToken.bind(manager)
    manager.parseHTMLToken = (token) => {
      const parsed = parseHTML(token)
      const html = token.text || token.raw || ''
      return keepsWords(html, parsed) ? parsed : manager.htmlAsLiteralText(html, !!token.block)
    }
    // a link definition (`[name]: address`) has no place in the editor and
    // upstream drops it, so one that no link uses left the note: it stays
    // as its text
    const fallback = manager.parseFallbackToken.bind(manager)
    manager.parseFallbackToken = (token, ...rest) =>
      token.type === 'def'
        ? manager.htmlAsLiteralText(token.raw ?? '', true)
        : fallback(token, ...rest)
    // a link with no text has nothing to carry it and was dropped with its
    // address: it shows the address instead
    for (const spec of manager.registry.get('link') ?? []) {
      const parseLink = spec.parseMarkdown
      if (!parseLink) continue
      spec.parseMarkdown = (token, helpers) => {
        const href = token.href ?? ''
        const text = [{ type: 'text', raw: href, text: href }]
        return parseLink(
          token.tokens?.length || !href ? token : { ...token, tokens: text },
          helpers,
        )
      }
    }
    if (initial) {
      const json = manager.parse(options.content as string)
      if (json.content.length) options.content = json
    }
    // a link is written knowing only its attributes: those of the links
    // showing their own address are noted as their line is written
    const ownAddress = new WeakSet<object>()
    const renderInline = manager.renderNodesWithMarkBoundaries.bind(manager)
    manager.renderNodesWithMarkBoundaries = (nodes, ...rest) => {
      for (const node of nodes) {
        const mark = node.marks?.[0]
        if (showsItsAddress(node) && typeof mark === 'object' && mark.attrs)
          ownAddress.add(mark.attrs)
      }
      return renderInline(nodes, ...rest)
    }
    for (const spec of manager.nodeTypeRegistry.get('link') ?? []) {
      spec.renderMarkdown = (node, helpers) => {
        const text = helpers.renderChildren(node)
        if (node.attrs && ownAddress.has(node.attrs)) return `<${text}>`
        const href = linkDestination(node.attrs?.href ?? '')
        const title = node.attrs?.title?.replace(/"/g, '\\"')
        return title ? `[${text}](${href} "${title}")` : `[${text}](${href})`
      }
    }
    // an item whose first line is empty is written with its marker alone:
    // "- " with its space and nothing after reads as text, and the code or
    // list the item held after it went with the next save
    for (const spec of manager.nodeTypeRegistry.get('listItem') ?? []) {
      const render = spec.renderMarkdown as ((...args: unknown[]) => string) | undefined
      if (!render) continue
      spec.renderMarkdown = (...args: unknown[]) =>
        render(...args).replace(/^([-+*]|\d{1,9}[.)]) +(?=\n|$)/, '$1')
    }
    const inCode = (node: JSONLike, parent?: JSONLike) =>
      (parent?.type !== undefined && manager.codeTypes.has(parent.type)) ||
      (node.marks ?? []).some((m) => manager.codeTypes.has(typeof m === 'string' ? m : m.type))
    // nothing is escaped between < and >, a backslash would be part of the address
    manager.encodeTextForMarkdown = (text, node, parent) =>
      inCode(node, parent) || showsItsAddress(node)
        ? text
        : escapeHtmlLookalikes(manager.escapeMarkdownSyntax(text))
  },
})
