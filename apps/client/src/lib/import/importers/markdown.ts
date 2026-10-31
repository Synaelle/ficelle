// SPDX-License-Identifier: AGPL-3.0-or-later
// Any folder of markdown files (Typora, iA Writer, Notesnook, UpNote, an
// export of this app...): a note per file, folders as they are, front
// matter for the title, tags and dates, else a leading heading and the
// file's own date.

import {
  Index,
  NoteFiles,
  isMarkdown,
  metaList,
  metaText,
  parseDate,
  rewriteLinks,
  splitFrontMatter,
  takeHeading,
} from '../markdown'
import { baseName, dirName, readText, withoutExtension } from '../source'
import {
  NOTE_REF,
  noteRef,
  type Importer,
  type ImportModel,
  type ImportSource,
  type ModelFolder,
  type ModelNote,
  type Report,
} from '../types'

/** What an importer reads itself before the shared steps, all optional. */
export interface Prepared {
  /** The text with the app's own syntax turned into plain markdown. */
  text?: string
  title?: string
  tags?: string[]
  created?: number
  updated?: number
  pinned?: boolean
  trashed?: boolean
  /** A key steadier than the path (an id kept by the app). */
  key?: string
}

/** Folders of an export from the paths of its notes, keyed by path. */
export function foldersOf(
  paths: string[],
  skip: (dir: string) => boolean = () => false,
): ModelFolder[] {
  const dirs = new Set<string>()
  for (const path of paths) {
    for (let dir = dirName(path); dir && !dirs.has(dir); dir = dirName(dir)) {
      if (!skip(dir)) dirs.add(dir)
    }
  }
  return [...dirs]
    .sort((a, b) => a.split('/').length - b.split('/').length || a.localeCompare(b))
    .map((dir) => ({ key: dir, parent: dirName(dir) || null, name: baseName(dir) }))
}

/** The folder a note goes in: its own, or the nearest one kept. */
export function folderOf(path: string, kept: Set<string>): string | null {
  for (let dir = dirName(path); dir; dir = dirName(dir)) if (kept.has(dir)) return dir
  return null
}

/** Reads a folder of markdown notes: what the markdown family shares. */
export async function readMarkdownNotes(
  source: ImportSource,
  report: Report,
  options: {
    notes: string[]
    /** Inline `#tags` read from the text (Obsidian, Bear, Zettlr). */
    inlineTags?: boolean
    /** Folders that only hold files, not made as folders. */
    skipDir?: (dir: string) => boolean
    /** The app's own syntax and metadata, read before the shared steps. */
    prepare?: (text: string, path: string) => Prepared
    /** Other names notes are linked by (titles, ids), before reading. */
    aliases?: (index: Index) => void | Promise<void>
    /** Folders made otherwise than from the paths. */
    folders?: ModelFolder[]
    folderOf?: (path: string) => string | null
    /** The title when the note gives none, the file name by default. */
    titleOf?: (path: string) => string
  },
): Promise<ImportModel> {
  const noteSet = new Set(options.notes)
  const index = new Index(
    source.entries.map((e) => e.path),
    (path) => noteSet.has(path),
  )
  await options.aliases?.(index)
  const folders = options.folders ?? foldersOf(options.notes, options.skipDir)
  const kept = new Set(folders.map((f) => f.key))
  const modified = new Map(source.entries.map((e) => [e.path, e.modified]))
  const notes: ImportModel['notes'] = []
  // links name notes by path, an importer may key them otherwise (an id)
  const keyOf = new Map<string, string>()
  let done = 0
  for (const path of options.notes) {
    report.progress(done++, options.notes.length)
    let text: string
    try {
      text = (await readText(source, path)).replace(/\r\n?/g, '\n')
    } catch {
      report.add({ kind: 'unreadable', detail: path })
      continue
    }
    const own = options.prepare?.(text, path) ?? {}
    const { meta, body: rest } = splitFrontMatter(own.text ?? text)
    let title = own.title ?? metaText(meta.title).trim()
    let body = rest
    if (!title) {
      const heading = takeHeading(body)
      if (heading.title) {
        title = heading.title
        body = heading.body
      }
    }
    title ||= options.titleOf?.(path) ?? withoutExtension(baseName(path))
    const tags = [
      ...(own.tags ?? []),
      ...metaList(meta.tags),
      ...metaList(meta.tag),
      ...metaList(meta.keywords),
    ]
    if (options.inlineTags) tags.push(...inlineTags(body))
    const files = new NoteFiles()
    body = rewriteLinks(body, path, title, index, files, report)
    const fileTime = modified.get(path) ?? undefined
    const created =
      own.created ??
      parseDate(meta.created ?? meta.date ?? meta['date created'] ?? meta.creation_date)
    const updated =
      own.updated ??
      parseDate(meta.updated ?? meta.modified ?? meta['date modified'] ?? meta.lastmod)
    const note: ModelNote = {
      // an id in front matter (this app's export writes one) outlives renames
      key:
        own.key ?? (/^[0-9a-f-]{36}$/i.test(metaText(meta.id)) ? `id:${metaText(meta.id)}` : path),
      folder: options.folderOf ? options.folderOf(path) : folderOf(path, kept),
      title,
      body: body.replace(/^\n+/, '').trimEnd(),
      tags,
      created: created ?? updated ?? fileTime,
      updated: updated ?? fileTime ?? created,
      pinned: own.pinned ?? meta.pinned === 'true',
      trashed: own.trashed ?? false,
      files: files.files,
    }
    notes.push(note)
    keyOf.set(path, note.key)
  }
  report.progress(options.notes.length, options.notes.length)
  if ([...keyOf].some(([path, key]) => path !== key)) {
    for (const note of notes) {
      note.body = note.body.replace(NOTE_REF, (_all, key: string, fallback: string) =>
        noteRef(keyOf.get(key) ?? key, fallback),
      )
    }
  }
  return { folders, notes }
}

/**
 * Inline tags: `#tag`, `#a/b`, outside code and headings, not part of a
 * word or an address.
 */
export function inlineTags(body: string): string[] {
  const prose = body
    .replace(/(^|\n)(```|~~~)[^\n]*\n[\s\S]*?\n\2/g, '\n')
    .replace(/`[^`\n]*`/g, '')
    .replace(/\[[^\]\n]*\]\([^)\n]*\)/g, '')
  const tags: string[] = []
  for (const match of prose.matchAll(/(^|[\s(,;])#([\p{L}\p{N}_][\p{L}\p{N}_/-]*)/gu)) {
    const tag = (match[2] ?? '').replace(/[/-]+$/, '')
    // #123 is an issue number, not a tag
    if (tag && !/^\d+$/.test(tag)) tags.push(tag)
  }
  return tags
}

export const markdownImporter: Importer = {
  id: 'markdown',
  app: 'Markdown',
  picks: ['folder', 'file'],
  extensions: ['zip', 'md', 'markdown', 'tar'],
  detect: (source) => (source.entries.some((e) => isMarkdown(e.path)) ? 0.3 : 0),
  read: (source, report) =>
    readMarkdownNotes(source, report, {
      notes: source.entries.map((e) => e.path).filter((p) => isMarkdown(p) && !hidden(p)),
      skipDir: (dir) => hidden(dir),
    }),
}

/** Dot folders (`.obsidian`, `.git`, `.trash`) are an app's own, not notes. */
export const hidden = (path: string) => path.split('/').some((part) => part.startsWith('.'))
