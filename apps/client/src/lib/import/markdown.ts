// SPDX-License-Identifier: AGPL-3.0-or-later
// What the markdown family shares (plain folders, Obsidian, Zettlr, Logseq,
// Bear, Notion's markdown): front matter, and links to notes and files
// written as wiki links or markdown links, resolved inside the export.

import { baseName, extension, mimeOf, resolvePath, withoutExtension } from './source'
import { fileRef, noteRef, type ModelFile, type Report } from './types'

export const MARKDOWN = new Set(['md', 'markdown', 'mdown', 'mkd', 'txt'])
export const isMarkdown = (path: string) =>
  MARKDOWN.has(extension(path)) && extension(path) !== 'txt'

const FRONT_MATTER = /^---[ \t]*\n([\s\S]*?)\n(?:---|\.\.\.)[ \t]*(?:\n|$)/

const ESCAPES: Record<string, string> = {
  n: '\n',
  t: '\t',
  r: '\r',
  '"': '"',
  '\\': '\\',
  '/': '/',
  '0': '\0',
}

/** An inline list's items, commas inside quotes kept. */
export function splitInline(list: string): string[] {
  const items: string[] = []
  let item = ''
  let quote: string | null = null
  for (let i = 0; i < list.length; i += 1) {
    const c = list[i] ?? ''
    if (quote) {
      item += c
      if (c === '\\' && quote === '"') item += list[++i] ?? ''
      else if (c === quote) quote = null
    } else if (c === '"' || c === "'") {
      quote = c
      item += c
    } else if (c === ',') {
      items.push(item)
      item = ''
    } else item += c
  }
  items.push(item)
  return items
}

/**
 * A YAML scalar's text: double quoted with its escapes (`\"`, `\n`,
 * `é`), single quoted with `''` for a quote, or plain.
 */
export function unquote(value: string): string {
  const text = value.trim()
  if (text.length >= 2 && text.startsWith('"') && text.endsWith('"')) {
    return text
      .slice(1, -1)
      .replace(/\\(u[0-9a-fA-F]{4}|U[0-9a-fA-F]{8}|x[0-9a-fA-F]{2}|.)/g, (all, code: string) => {
        if (/^[uUx]/.test(code)) return String.fromCodePoint(parseInt(code.slice(1), 16))
        return ESCAPES[code] ?? all
      })
  }
  if (text.length >= 2 && text.startsWith("'") && text.endsWith("'")) {
    return text.slice(1, -1).replace(/''/g, "'")
  }
  return text
}

/**
 * The simple YAML of front matter: `key: value`, `key: [a, b]` and block
 * lists. Nested maps and multi-line strings are left out.
 */
export function splitFrontMatter(text: string): {
  meta: Record<string, string | string[]>
  body: string
} {
  const front = FRONT_MATTER.exec(text)
  if (!front) return { meta: {}, body: text }
  const meta: Record<string, string | string[]> = {}
  let list: string[] | null = null
  for (const line of (front[1] ?? '').split('\n')) {
    const item = /^\s*-\s+(.*)$/.exec(line)
    if (item && list) {
      list.push(unquote(item[1] ?? ''))
      continue
    }
    const field = /^([A-Za-z_][\w -]*?)\s*:\s?(.*)$/.exec(line)
    if (!field) {
      list = null
      continue
    }
    const key = (field[1] ?? '').toLowerCase()
    const raw = (field[2] ?? '').trim()
    if (raw === '') {
      list = []
      meta[key] = list
    } else if (/^\[.*\]$/.test(raw)) {
      list = null
      meta[key] = splitInline(raw.slice(1, -1)).map(unquote).filter(Boolean)
    } else {
      list = null
      meta[key] = unquote(raw)
    }
  }
  return { meta, body: text.slice(front[0].length) }
}

export const metaText = (value: string | string[] | undefined) =>
  Array.isArray(value) ? value.join(' ') : (value ?? '')

export const metaList = (value: string | string[] | undefined) =>
  Array.isArray(value) ? value : (value ?? '').split(/[,\s]+/).filter(Boolean)

/** A date of front matter or an export, as unix milliseconds. */
export function parseDate(value: string | string[] | undefined): number | undefined {
  const text = metaText(value).trim()
  if (!text) return undefined
  // 20250115T103000Z (Evernote), 20250115103000 (Zettlr ids)
  const compact = /^(\d{4})(\d{2})(\d{2})T?(\d{2})(\d{2})(\d{2})(Z?)$/.exec(text)
  if (compact) {
    const [, y, mo, d, h, mi, s] = compact.map(Number)
    return Date.UTC(y ?? 0, (mo ?? 1) - 1, d ?? 1, h ?? 0, mi ?? 0, s ?? 0)
  }
  // a date alone is that day at midnight, local time, as people mean it
  const day = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text)
  if (day) return new Date(Number(day[1]), Number(day[2]) - 1, Number(day[3])).getTime()
  const time = Date.parse(text.replace(/^(\d{4}-\d{2}-\d{2}) (\d)/, '$1T$2'))
  return Number.isFinite(time) ? time : undefined
}

/** A leading `# Title` line, taken out of the body. */
export function takeHeading(body: string): { title: string | null; body: string } {
  const trimmed = body.replace(/^\s*\n/, '')
  const heading = /^[ \t]*#[ \t]+(.+?)[ \t#]*(?:\n|$)/.exec(trimmed)
  if (!heading) return { title: null, body }
  return { title: heading[1] ?? '', body: trimmed.slice(heading[0].length).replace(/^\n+/, '') }
}

/** Finds the notes and files of an export by the names links use. */
export class Index {
  private notes = new Map<string, string[]>()
  private files = new Map<string, string[]>()
  private paths = new Set<string>()
  private aliases = new Map<string, string>()

  /** Another name a note is linked by: its title, a page name, an id. */
  alias(name: string, path: string) {
    const key = name.trim().toLowerCase()
    if (key && !this.aliases.has(key)) this.aliases.set(key, path)
  }

  constructor(paths: string[], isNote: (path: string) => boolean) {
    for (const path of paths) {
      this.paths.add(path)
      const map = isNote(path) ? this.notes : this.files
      const add = (key: string) => map.set(key, [...(map.get(key) ?? []), path])
      add(baseName(path).toLowerCase())
      if (isNote(path)) add(withoutExtension(baseName(path)).toLowerCase())
    }
  }

  /** Several matches: the one nearest the linking note, then the shortest. */
  private best(found: string[] | undefined, from: string): string | null {
    if (!found?.length) return null
    const dir = from.slice(0, from.lastIndexOf('/') + 1)
    return (
      [...found].sort((a, b) => {
        const near = Number(b.startsWith(dir)) - Number(a.startsWith(dir))
        return near || a.split('/').length - b.split('/').length || a.length - b.length
      })[0] ?? null
    )
  }

  /** A wiki link's target: `Note`, `folder/Note`, `Note.md`. */
  note(target: string, from: string): string | null {
    const clean = target.trim()
    if (!clean) return null
    const withExt = isMarkdown(clean) ? clean : `${clean}.md`
    const relative = resolvePath(from, withExt)
    if (relative && this.paths.has(relative)) return relative
    if (this.paths.has(withExt)) return withExt
    const named = this.aliases.get(clean.toLowerCase())
    if (named) return named
    const candidates = this.notes.get(baseName(clean).toLowerCase())
    const suffix = `/${withExt.toLowerCase()}`
    const exact = candidates?.filter((p) => `/${p.toLowerCase()}`.endsWith(suffix))
    return this.best(exact?.length ? exact : candidates, from)
  }

  /** A file's target: a path relative to the note, from the top, or a bare name. */
  file(target: string, from: string): string | null {
    const relative = resolvePath(from, target)
    if (relative && this.paths.has(relative)) return relative
    let decoded = target
    try {
      decoded = decodeURIComponent(target)
    } catch {
      // kept as written
    }
    if (this.paths.has(decoded)) return decoded
    return this.best(this.files.get(baseName(decoded).toLowerCase()), from)
  }

  isNote(path: string): boolean {
    return this.notes.get(baseName(path).toLowerCase())?.includes(path) ?? false
  }
}

/** Splits text into code (left alone) and the rest, in order. */
function outsideCode(text: string, change: (prose: string) => string): string {
  const code = /(^|\n)(```|~~~)[^\n]*\n[\s\S]*?(\n\2[^\n]*(?=\n|$)|$)|`[^`\n]+`/g
  let out = ''
  let at = 0
  for (const match of text.matchAll(code)) {
    out += change(text.slice(at, match.index)) + match[0]
    at = (match.index ?? 0) + match[0].length
  }
  return out + change(text.slice(at))
}

/** Collects the files a note uses, one each, named after their entry. */
export class NoteFiles {
  readonly files: ModelFile[] = []
  private byEntry = new Map<string, string>()

  add(entry: string, name = baseName(entry)): string {
    const known = this.byEntry.get(entry)
    if (known) return known
    const free = this.free(name)
    this.byEntry.set(entry, free)
    this.files.push({ name: free, mime: mimeOf(name), entry })
    return free
  }

  /** A file the importer decoded (a data url, base64 in an export). */
  addBytes(name: string, mime: string, bytes: Uint8Array): string {
    const free = this.free(name)
    this.files.push({ name: free, mime, bytes })
    return free
  }

  private free(name: string): string {
    let free = name
    for (let n = 2; this.files.some((f) => f.name === free); n += 1) {
      free = name.replace(/(\.[^.]*)?$/, ` ${n}$1`)
    }
    return free
  }
}

const isWeb = (target: string) => /^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('#')

/**
 * Rewrites a note's links: wiki links and embeds (`[[Note]]`, `![[img.png|300]]`)
 * and markdown links to other notes or files of the export. Notes become
 * {@link noteRef}, files {@link fileRef} added to `files`.
 */
export function rewriteLinks(
  body: string,
  from: string,
  title: string,
  index: Index,
  files: NoteFiles,
  report: Report,
): string {
  const fileMarkdown = (entry: string, label: string, image: boolean, width?: string) => {
    const name = files.add(entry)
    const isImage = image && mimeOf(entry).startsWith('image/')
    const layout = isImage && width ? `{width=${width}}` : ''
    return isImage
      ? `![${label}](${fileRef(name)})${layout}`
      : `[${label || name}](${fileRef(name)})`
  }
  return outsideCode(body, (prose) =>
    prose
      // wiki links and embeds
      .replace(
        /(!?)\[\[([^\]|#\n]*)(#[^\]|\n]*)?(?:\|([^\]\n]*))?\]\]/g,
        (all, bang: string, target: string, heading = '', alias?: string) => {
          if (!target.trim()) return all
          const note = index.note(target, from)
          if (note) {
            if (bang) report.add({ kind: 'simplified', note: title, detail: all })
            return `[[${noteRef(note, withoutExtension(baseName(target.trim())))}${heading}${alias ? `|${alias}` : ''}]]`
          }
          const file = index.file(target.trim(), from)
          if (file) {
            const width = /^\d+(x\d+)?$/.test(alias ?? '') ? alias?.split('x')[0] : undefined
            const label = width || !alias ? withoutExtension(baseName(file)) : alias
            return fileMarkdown(file, bang ? label : alias || baseName(file), !!bang, width)
          }
          if (bang && extension(target)) {
            report.add({ kind: 'fileMissing', note: title, detail: target.trim() })
            return all
          }
          // a link to a note not in the export stays a link, by its name
          return `[[${noteRef(`missing:${target.trim()}`, target.trim())}${heading}${alias ? `|${alias}` : ''}]]`
        },
      )
      // markdown links and images, <path> or a path with spaces and
      // brackets as Obsidian writes them
      .replace(
        /(!?)\[([^\]\n]*)\]\((?:<([^>\n]+)>|((?:[^()\n"]|\([^()\n]*\))+?))(\s+"[^"\n]*")?\)/g,
        (all, bang: string, label: string, angled?: string, plain?: string) => {
          const target = (angled ?? plain ?? '').trim()
          // already a file of the note, from an embed above
          if (isWeb(target) || /[\uE000-\uE004]/.test(target)) return all
          const [path, anchor = ''] = target.split('#') as [string, string?]
          const resolved = resolvePath(from, path)
          if (resolved && index.isNote(resolved)) {
            const heading = anchor ? `#${decodeURIComponent(anchor).replace(/-/g, ' ')}` : ''
            const fallback = withoutExtension(baseName(resolved))
            return `[[${noteRef(resolved, fallback)}${heading}${label && label !== fallback ? `|${label.replace(/[|\]]/g, '')}` : ''}]]`
          }
          const file = index.file(path, from)
          if (file && !index.isNote(file)) return fileMarkdown(file, label, !!bang)
          if (path && !isWeb(path)) {
            report.add({ kind: bang ? 'fileMissing' : 'linkMissing', note: title, detail: target })
          }
          return all
        },
      ),
  )
}
