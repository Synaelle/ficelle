// SPDX-License-Identifier: AGPL-3.0-or-later
// The importer interface. An importer knows one app's export and nothing
// else: it reads a source into the neutral model below, and the pipeline
// (pipeline.ts) does the rest for every format alike: links, titles, file
// names, what is already in, writing, the report.

/** One file of an export, as the app lists it. */
export interface SourceEntry {
  /** Path inside the export, with `/`. */
  path: string
  size: number
  /** Last modified, unix milliseconds, when known. */
  modified: number | null
}

/** An export opened for reading: a folder, an archive or a file. */
export interface ImportSource {
  /** The file or folder name the person picked. */
  name: string
  entries: SourceEntry[]
  /** Set when the app reads the export: files then go straight from it. */
  nativeId: number | null
  read(path: string): Promise<Uint8Array>
}

export interface ModelFolder {
  key: string
  /** The parent's key, null for the top of the import. */
  parent: string | null
  name: string
}

/** A file of a note: an entry of the export, or bytes the importer decoded. */
export interface ModelFile {
  /** Name the body refers to it by with {@link fileRef}. */
  name: string
  mime: string
  entry?: string
  bytes?: Uint8Array
}

export interface ModelNote {
  /** The same each time the same export is read: skips what is already in. */
  key: string
  folder: string | null
  title: string
  /** Markdown, with {@link noteRef} and {@link fileRef} where it points. */
  body: string
  tags: string[]
  created?: number
  updated?: number
  pinned?: boolean
  trashed?: boolean
  files: ModelFile[]
}

export interface ImportModel {
  folders: ModelFolder[]
  notes: ModelNote[]
}

/** What could not be brought in as it was, by kind. */
export type ReportKind =
  /** A link to a note the export does not have. */
  | 'linkMissing'
  /** A file a note uses that the export does not have. */
  | 'fileMissing'
  /** A file the rules of the server do not allow, or unreadable. */
  | 'fileLeftOut'
  /** Content with no equivalent here, dropped (scripts, unknown blocks). */
  | 'dropped'
  /** Encrypted content that cannot be opened here. */
  | 'encrypted'
  /** Kept in a simpler form (a database flattened to its pages). */
  | 'simplified'
  /** An item of the export that could not be read at all. */
  | 'unreadable'

export interface ReportItem {
  kind: ReportKind
  /** Title of the note concerned, when there is one. */
  note?: string
  /** What exactly: a link target, a file name, an element. */
  detail?: string
}

export interface Report {
  add(item: ReportItem): void
  /** Lets the dialog show how far reading is. */
  progress(done: number, total: number): void
}

/** How the person gets the export out of the app. */
export type Pick = 'folder' | 'file'

export interface Importer {
  /** Stable id, kept with each imported note (`notion`, `joplin`...). */
  id: string
  /** The app's own name. */
  app: string
  /** What to pick: a folder, or a file (an archive or an export file). */
  picks: Pick[]
  /** File extensions it reads, for the file picker. */
  extensions: string[]
  /** How sure it is that the source is its format, 0 to 1. */
  detect(source: ImportSource): number
  read(source: ImportSource, report: Report): Promise<ImportModel>
}

// Markers in a body until the pipeline knows the final titles and file
// names. Private use characters: no export has them in its text.
const NOTE_START = '\uE000'
const NOTE_FALLBACK = '\uE001'
const NOTE_END = '\uE002'
const FILE_START = '\uE003'
const FILE_END = '\uE004'

export const NOTE_REF = new RegExp(
  `${NOTE_START}([^${NOTE_FALLBACK}]*)${NOTE_FALLBACK}([^${NOTE_END}]*)${NOTE_END}`,
  'g',
)
export const FILE_REF = new RegExp(`${FILE_START}([^${FILE_END}]*)${FILE_END}`, 'g')

/**
 * Stands for another note's title inside `[[...]]`. `fallback` is what is
 * left when the export does not have that note.
 */
export function noteRef(key: string, fallback: string): string {
  const clean = (text: string) => text.replace(/[\uE000-\uE004]/g, '')
  return `${NOTE_START}${clean(key)}${NOTE_FALLBACK}${clean(fallback)}${NOTE_END}`
}

/** Stands for the address of a note's file, in `![...](...)` or `[...](...)`. */
export function fileRef(name: string): string {
  return `${FILE_START}${name.replace(/[\uE000-\uE004]/g, '')}${FILE_END}`
}
