// SPDX-License-Identifier: AGPL-3.0-or-later
// Obsidian, Zettlr and Apple Notes' markdown exports: folders of markdown
// with each app's own habits on top of what the markdown family shares.

import { isMarkdown, type Index } from '../markdown'
import { baseName, extension } from '../source'
import type { Importer, ImportSource } from '../types'
import { hidden, readMarkdownNotes } from './markdown'

const notesOf = (source: ImportSource, also: (path: string) => boolean = () => false) =>
  source.entries.map((e) => e.path).filter((p) => (isMarkdown(p) || also(p)) && !hidden(p))

/**
 * The vault folder. Properties in front matter, inline `#tags`, `[[links]]`
 * found by the shortest path, attachments wherever the vault keeps them.
 * Its trash (`.trash`) and settings (`.obsidian`) are left out.
 */
export const obsidianImporter: Importer = {
  id: 'obsidian',
  app: 'Obsidian',
  picks: ['folder', 'file'],
  extensions: ['zip'],
  detect: (source) =>
    source.entries.some((e) => e.path.startsWith('.obsidian/') || e.path.includes('/.obsidian/'))
      ? 0.95
      : 0,
  read: (source, report) =>
    readMarkdownNotes(source, report, {
      notes: notesOf(source),
      inlineTags: true,
      skipDir: hidden,
    }),
}

/** A Zettlr id: the date and time a note was made, `YYYYMMDDHHMMSS`. */
const ZETTLR_ID = /^(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})(?!\d)/

function idTime(path: string): number | undefined {
  const id = ZETTLR_ID.exec(baseName(path))
  if (!id) return undefined
  const [, y, mo, d, h, mi, s] = id.map(Number)
  const time = new Date(y ?? 0, (mo ?? 1) - 1, d ?? 1, h ?? 0, mi ?? 0, s ?? 0).getTime()
  return Number.isFinite(time) ? time : undefined
}

/**
 * Zettlr's folders: YAML `title` and `keywords`, inline tags, notes linked
 * by their id (`[[20250115103000]]`) as often as by their name.
 */
export const zettlrImporter: Importer = {
  id: 'zettlr',
  app: 'Zettlr',
  picks: ['folder', 'file'],
  extensions: ['zip'],
  detect: (source) => {
    if (source.entries.some((e) => baseName(e.path) === '.ztr-directory')) return 0.9
    const notes = notesOf(source)
    const withIds = notes.filter((p) => ZETTLR_ID.test(baseName(p))).length
    return notes.length && withIds / notes.length >= 0.5 ? 0.5 : 0
  },
  read: (source, report) =>
    readMarkdownNotes(source, report, {
      notes: notesOf(source),
      inlineTags: true,
      skipDir: hidden,
      aliases: (index: Index) => {
        for (const path of notesOf(source)) {
          const id = ZETTLR_ID.exec(baseName(path))?.[0]
          if (id) index.alias(id, path)
        }
      },
      prepare: (_text, path) => ({ created: idTime(path) }),
    }),
}

/**
 * Apple Notes: the markdown export of macOS and iOS 26 (a file per note,
 * pictures in `Attachments/`), or the copy of iCloud data from Apple's
 * privacy site (a text file per note). Neither keeps dates or tags.
 */
export const appleNotesImporter: Importer = {
  id: 'apple-notes',
  app: 'Apple Notes',
  picks: ['folder', 'file'],
  extensions: ['zip'],
  detect: (source) => {
    const paths = source.entries.map((e) => e.path)
    if (paths.some((p) => /(^|\/)iCloud Notes\//.test(p))) return 0.8
    const attachments = paths.some((p) => /(^|\/)Attachments\//.test(p))
    return attachments && paths.some(isMarkdown) ? 0.4 : 0
  },
  read: (source, report) =>
    readMarkdownNotes(source, report, {
      notes: notesOf(source, (p) => extension(p) === 'txt' && !/(^|\/)Attachments\//.test(p)),
      skipDir: (dir) => hidden(dir) || /(^|\/)Attachments$/.test(dir),
    }),
}
