// SPDX-License-Identifier: AGPL-3.0-or-later
// The importers the app offers, and exports opened by the app.

import { backend } from '../backend'
import { bearImporter } from './importers/bear'
import { evernoteImporter } from './importers/evernote'
import { dayOneImporter, opmlImporter, roamImporter, tomboyImporter } from './importers/extras'
import { htmlImporter, notionHtmlImporter } from './importers/html'
import { joplinImporter } from './importers/joplin'
import { simplenoteImporter, standardNotesImporter } from './importers/json'
import { keepImporter } from './importers/keep'
import { logseqImporter } from './importers/logseq'
import { markdownImporter } from './importers/markdown'
import { notionImporter as notionMarkdown } from './importers/notion'
import { appleNotesImporter, obsidianImporter, zettlrImporter } from './importers/obsidian'
import type { Importer, ImportSource } from './types'

/** Notion's two exports, markdown and HTML, as one choice. */
export const notionImporter: Importer = {
  ...notionMarkdown,
  detect: (source) => Math.max(notionMarkdown.detect(source), notionHtmlImporter.detect(source)),
  read: (source, report) =>
    notionHtmlImporter.detect(source) > 0
      ? notionHtmlImporter.read(source, report)
      : notionMarkdown.read(source, report),
}

/** In the order shown. A new format is one file and one line here. */
export const IMPORTERS: Importer[] = [
  obsidianImporter,
  notionImporter,
  evernoteImporter,
  joplinImporter,
  keepImporter,
  logseqImporter,
  bearImporter,
  appleNotesImporter,
  simplenoteImporter,
  standardNotesImporter,
  zettlrImporter,
  dayOneImporter,
  roamImporter,
  opmlImporter,
  tomboyImporter,
  htmlImporter,
  markdownImporter,
]

/** Opens a folder, an archive or a file picked by the person. */
export async function openSource(path: string): Promise<ImportSource> {
  const opened = await backend.importOpen(path)
  return {
    name: opened.name,
    entries: opened.entries,
    nativeId: opened.id,
    read: (entry) => backend.importRead(opened.id, entry),
  }
}

export async function closeSource(source: ImportSource) {
  if (source.nativeId !== null) await backend.importClose(source.nativeId)
}
