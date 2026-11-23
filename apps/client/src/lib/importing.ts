// SPDX-License-Identifier: AGPL-3.0-or-later
// Turns markdown files from other apps into notes. Notes keep their title
// apart from the body, so a leading front matter title or level one heading
// becomes the title instead of staying in the text twice.

import { splitInline, unquote } from './import/markdown'
import { WINDOWS } from './platform'

export interface ImportedNote {
  title: string
  body: string
  tags: string[]
  /** Front matter's `pinned: true`, as Ficelle's own export writes it. */
  pinned: boolean
}

const MARKDOWN_FILE = /\.(md|markdown|mdown|mkd)$/i

/** The file inputs' filter. Windows' dialog lists the extension a type
 * stands for again after the extensions, so it gets them alone. */
export const MARKDOWN_ACCEPT = WINDOWS
  ? '.md,.markdown,.mdown,.mkd'
  : '.md,.markdown,.mdown,.mkd,text/markdown'

export function isMarkdownFile(file: { name: string }): boolean {
  return MARKDOWN_FILE.test(file.name)
}

// Obsidian and static site generators put metadata in a yaml block at the top
const FRONT_MATTER = /^---\n([\s\S]*?)\n---[ \t]*(?:\n|$)/
const LEADING_HEADING = /^[ \t]*#[ \t]+(.+?)[ \t#]*(?:\n|$)/

/** Reads `title`, `tags` and `pinned` from front matter, only the common simple forms. */
function readFrontMatter(yaml: string): { title?: string; tags: string[]; pinned: boolean } {
  const lines = yaml.split('\n')
  let title: string | undefined
  let pinned = false
  const tags: string[] = []
  lines.forEach((line, i) => {
    const field = /^(title|tags|pinned):(.*)$/.exec(line)
    if (!field) return
    const [, key, raw = ''] = field
    if (key === 'pinned') {
      pinned = raw.trim() === 'true'
      return
    }
    if (key === 'title') {
      title = unquote(raw) || undefined
      return
    }
    const inline = raw.trim().replace(/^\[(.*)\]$/, '$1')
    if (inline) {
      tags.push(...splitInline(inline).map(unquote))
      return
    }
    // block list: following lines that start with a dash
    for (const item of lines.slice(i + 1)) {
      const entry = /^\s*-\s+(.*)$/.exec(item)
      if (!entry) break
      tags.push(unquote(entry[1] ?? ''))
    }
  })
  return { title, tags: tags.map((tag) => tag.replace(/^#/, '')).filter(Boolean), pinned }
}

export function parseMarkdownFile(fileName: string, text: string): ImportedNote {
  let body = text.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n')
  let title: string | undefined
  let tags: string[] = []
  let pinned = false

  const front = FRONT_MATTER.exec(body)
  if (front) {
    const meta = readFrontMatter(front[1] ?? '')
    title = meta.title
    tags = meta.tags
    pinned = meta.pinned
    body = body.slice(front[0].length)
  }
  if (!title) {
    const heading = LEADING_HEADING.exec(body.replace(/^\n+/, ''))
    if (heading) {
      title = heading[1]
      body = body.replace(/^\n+/, '').slice(heading[0].length)
    }
  }
  title ??= fileName.replace(MARKDOWN_FILE, '')
  return { title: title.trim(), body: body.replace(/^\n+/, ''), tags, pinned }
}

/** Characters markdown, or one of the editor's own marks, would read as
 * formatting anywhere in a line. */
const INLINE_MARKS = /[\\`*_[\]<>~|$=&]/g
/** A line start markdown reads as a heading, list or block. */
const LINE_START = /^(#|[-+]|:::)/
/** A numbered list's start: the escape goes before its dot. */
const NUMBER_START = /^(\d{1,9})([.)])/

/**
 * A plain text file as a note: the file's name is the title, and the
 * markdown shows the text as it was written. Formatting characters are
 * escaped, each line break stays one (a hard break) and indents become
 * spaces that never start a code block. Runs of empty lines become one, as
 * markdown keeps no more.
 */
export function parseTextFile(fileName: string, text: string): ImportedNote {
  const line = (raw: string) => {
    const indent = /^[ \t]*/.exec(raw)?.[0] ?? ''
    const rest = raw
      .slice(indent.length)
      .replace(INLINE_MARKS, '\\$&')
      .replace(LINE_START, '\\$1')
      .replace(NUMBER_START, '$1\\$2')
    return indent.replace(/\t/g, '    ').replace(/ /g, '\u00a0') + rest
  }
  const body = text
    .replace(/^\uFEFF/, '')
    .replace(/\r\n?/g, '\n')
    .split(/\n[ \t]*\n\s*/)
    .map((paragraph) => paragraph.replace(/\s+$/, ''))
    .filter(Boolean)
    .map((paragraph) => paragraph.split('\n').map(line).join('\\\n'))
    .join('\n\n')
  return { title: fileName.replace(/\.txt$/i, '').trim(), body, tags: [], pinned: false }
}
