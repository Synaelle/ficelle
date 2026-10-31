// SPDX-License-Identifier: AGPL-3.0-or-later
// One pipeline for every importer: pick the format, read the export into
// the model, settle titles, links and file names, leave out what an earlier
// import brought in, then write a batch at a time into a folder of its own.

import { ATTACHMENT_PREFIX } from '../attachments'
import type { Backend, Id } from '../backend'
import type { ImportFileInput, ImportNoteInput } from '../backend/types'
import {
  FILE_REF,
  NOTE_REF,
  type Importer,
  type ImportModel,
  type ImportSource,
  type ModelFile,
  type ModelNote,
  type ReportItem,
} from './types'

/** Notes per batch: each batch is one transaction with its sync record. */
export const BATCH = 100

/** The importer most sure the source is its format, null when none is. */
export function detect(source: ImportSource, importers: Importer[]): Importer | null {
  let best: Importer | null = null
  let score = 0
  for (const importer of importers) {
    const sure = importer.detect(source)
    if (sure > score) {
      best = importer
      score = sure
    }
  }
  return best
}

/** How a file name goes in a markdown link: spaces and brackets escaped. */
export const fileAddress = (name: string) =>
  ATTACHMENT_PREFIX + encodeURI(name).replace(/\(/g, '%28').replace(/\)/g, '%29')

/** A title `[[...]]` can point at: core links stop at these characters. */
const linkable = (title: string) => title.trim() !== '' && !/[[\]|#\n]/.test(title)

const titleKey = (title: string) => title.trim().toLowerCase()

/** `name`, else `name 2`, `name 3`... before the extension, as the core does. */
function freeName(name: string, taken: Set<string>): string {
  let free = name
  for (let n = 2; taken.has(free); n += 1) free = name.replace(/(\.[^.]*)?$/, ` ${n}$1`)
  return free
}

function cleanTags(tags: string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const raw of tags) {
    const tag = raw.trim().replace(/^#+/, '').trim()
    if (tag && !seen.has(tag.toLowerCase())) {
      seen.add(tag.toLowerCase())
      out.push(tag)
    }
  }
  return out
}

/**
 * Settles what the importer left open: titles made unique so `[[Title]]`
 * finds the right note, links and file addresses written out, tags cleaned.
 * `entries` are the export's paths, to tell files it does not have.
 */
export function settle(
  model: ImportModel,
  entries: Set<string>,
  report: ReportItem[],
): ImportModel {
  const titles = new Map<string, string>()
  const used = new Set<string>()
  for (const note of model.notes) {
    let title = note.title.replace(/\s+/g, ' ').trim()
    if (title) {
      const base = title
      for (let n = 2; used.has(titleKey(title)); n += 1) title = `${base} ${n}`
      used.add(titleKey(title))
    }
    titles.set(note.key, title)
  }

  const notes = model.notes.map((note): ModelNote => {
    const title = titles.get(note.key) ?? ''
    // files: one per entry, names unique in the note
    const files: ModelFile[] = []
    const names = new Map<string, string>()
    const taken = new Set<string>()
    for (const file of note.files) {
      if (names.has(file.name)) continue
      if (file.entry !== undefined && !entries.has(file.entry)) {
        report.push({ kind: 'fileMissing', note: title, detail: file.name })
        continue
      }
      if (file.entry === undefined && file.bytes === undefined) continue
      const name = freeName(file.name.replace(/[/\\]/g, '_').trim() || 'file', taken)
      taken.add(name)
      names.set(file.name, name)
      files.push({ ...file, name })
    }

    const body = note.body
      .replace(
        new RegExp(`\\[\\[${NOTE_REF.source}((?:#[^\\]|\\n]*)?)((?:\\|[^\\]\\n]*)?)\\]\\]`, 'g'),
        (_all, key: string, fallback: string, heading: string, alias: string) => {
          const target = titles.get(key)
          const label = alias.slice(1)
          if (target === undefined) {
            report.push({ kind: 'linkMissing', note: title, detail: fallback })
            return linkable(fallback) ? `[[${fallback}${heading}${alias}]]` : label || fallback
          }
          if (!linkable(target)) {
            report.push({ kind: 'simplified', note: title, detail: target })
            return label || target
          }
          // a label that says the title again is left out
          return `[[${target}${heading}${label === target ? '' : alias}]]`
        },
      )
      .replace(NOTE_REF, (_all, key: string, fallback: string) => titles.get(key) ?? fallback)
      .replace(FILE_REF, (_all, name: string) => {
        const final = names.get(name)
        if (final === undefined && !note.files.some((f) => f.name === name)) {
          report.push({ kind: 'fileMissing', note: title, detail: name })
        }
        return fileAddress(final ?? name)
      })

    return { ...note, title, body, tags: cleanTags(note.tags), files }
  })

  // parents before children, folders whose parent is unknown at the top
  const keys = new Set(model.folders.map((f) => f.key))
  const placed = new Set<string>()
  const folders: ImportModel['folders'] = []
  let left = model.folders.map((f) => (f.parent && keys.has(f.parent) ? f : { ...f, parent: null }))
  while (left.length) {
    const ready = left.filter((f) => f.parent === null || placed.has(f.parent))
    // a loop of parents: the rest go to the top
    const next = ready.length ? ready : left.map((f) => ({ ...f, parent: null }))
    for (const f of next) {
      if (placed.has(f.key)) continue
      placed.add(f.key)
      folders.push(f)
    }
    left = left.filter((f) => !placed.has(f.key))
  }
  return { folders, notes }
}

/** What an export holds, before anything is written. */
export interface Prepared {
  importer: Importer
  source: ImportSource
  model: ImportModel
  /** Keys of notes an earlier import brought in. */
  known: Set<string>
  report: ReportItem[]
  counts: { notes: number; folders: number; files: number; tags: number; skipped: number }
  /** Oldest and newest note dates, when the export has dates. */
  dates: { from: number; to: number } | null
}

/** Reads an export with an importer and settles it, writing nothing. */
export async function prepare(
  source: ImportSource,
  importer: Importer,
  backend: Backend,
  progress: (done: number, total: number) => void = () => {},
): Promise<Prepared> {
  const report: ReportItem[] = []
  const read = await importer.read(source, { add: (item) => report.push(item), progress })
  const model = settle(read, new Set(source.entries.map((e) => e.path)), report)
  const keys = model.notes.map((n) => n.key)
  const known = new Set<string>()
  for (let i = 0; i < keys.length; i += 1000) {
    for (const key of await backend.importKnown(importer.id, keys.slice(i, i + 1000)))
      known.add(key)
  }
  const fresh = model.notes.filter((n) => !known.has(n.key))
  const times = fresh.flatMap((n) => [n.created, n.updated]).filter((t): t is number => !!t)
  return {
    importer,
    source,
    model,
    known,
    report,
    counts: {
      notes: fresh.length,
      folders: model.folders.length,
      files: fresh.reduce((sum, n) => sum + n.files.length, 0),
      tags: new Set(fresh.flatMap((n) => n.tags.map((t) => t.toLowerCase()))).size,
      skipped: model.notes.length - fresh.length,
    },
    dates: times.length ? { from: Math.min(...times), to: Math.max(...times) } : null,
  }
}

export interface Written {
  root: Id
  notes: number
  skipped: number
  /** Stopped before the end: what was written stays. */
  cancelled: boolean
  report: ReportItem[]
}

/** Folders the notes to write are in, with their parents. */
function foldersNeeded(model: ImportModel, notes: ModelNote[]) {
  const byKey = new Map(model.folders.map((f) => [f.key, f]))
  const needed = new Set<string>()
  for (const note of notes) {
    for (
      let key = note.folder;
      key !== null && !needed.has(key);
      key = byKey.get(key)?.parent ?? null
    ) {
      if (!byKey.has(key)) break
      needed.add(key)
    }
  }
  return model.folders.filter((f) => needed.has(f.key))
}

async function fileInput(
  source: ImportSource,
  backend: Backend,
  file: ModelFile,
): Promise<ImportFileInput> {
  const { name, mime } = file
  if (file.bytes) return { name, mime, ...(await backend.importPut(file.bytes)) }
  const entry = file.entry ?? ''
  // the app reads files of its own sources straight from the export
  if (source.nativeId !== null) return { name, mime, entry }
  return { name, mime, ...(await backend.importPut(await source.read(entry))) }
}

/**
 * Writes what was prepared into a new folder named `name`, a batch at a
 * time. Stopping (the signal) keeps what is written.
 */
export async function write(
  prepared: Prepared,
  backend: Backend,
  options: { name: string; signal?: AbortSignal; progress?: (done: number, total: number) => void },
): Promise<Written> {
  const { source, model, known, importer } = prepared
  const report = [...prepared.report]
  const notes = model.notes.filter((n) => !known.has(n.key))
  const root = await backend.importBegin(source.nativeId, options.name)
  let folders = foldersNeeded(model, notes)
  let done = 0
  for (let i = 0; i < notes.length; i += BATCH) {
    if (options.signal?.aborted) break
    const batch: ImportNoteInput[] = []
    for (const note of notes.slice(i, i + BATCH)) {
      const sources: ImportFileInput[] = []
      for (const file of note.files) {
        try {
          sources.push(await fileInput(source, backend, file))
        } catch {
          report.push({ kind: 'fileLeftOut', note: note.title, detail: file.name })
        }
      }
      batch.push({
        key: note.key,
        folder: note.folder,
        title: note.title,
        body: note.body,
        tags: note.tags,
        created: note.created ?? null,
        updated: note.updated ?? null,
        pinned: note.pinned ?? false,
        trashed: note.trashed ?? false,
        sources,
      })
    }
    const written = await backend.importWrite(source.nativeId, {
      importer: importer.id,
      folders,
      notes: batch,
    })
    folders = []
    for (const left of written.leftOut) {
      report.push({ kind: 'fileLeftOut', note: left.note, detail: `${left.name} (${left.why})` })
    }
    done += batch.length
    options.progress?.(done, notes.length)
  }
  return {
    root,
    notes: done,
    skipped: known.size,
    cancelled: done < notes.length,
    report,
  }
}

/** The report as a note: counts, then each problem with its note. */
export function reportMarkdown(
  written: Written,
  labels: { summary: string; kinds: Record<string, string>; none: string },
): string {
  const lines = [labels.summary, '']
  if (!written.report.length) return [...lines, labels.none].join('\n')
  const kinds = [...new Set(written.report.map((r) => r.kind))]
  for (const kind of kinds) {
    const items = written.report.filter((r) => r.kind === kind)
    lines.push(`## ${labels.kinds[kind] ?? kind} (${items.length})`, '')
    for (const item of items) {
      const where = item.note ? `**${item.note.replace(/[*_[\]]/g, '\\$&')}**: ` : ''
      lines.push(`- ${where}${(item.detail ?? '').replace(/[[\]]/g, '\\$&')}`)
    }
    lines.push('')
  }
  return lines.join('\n').trimEnd()
}

/** A note's start as plain text for the preview: markers, links and marks read as words. */
export function plainSnippet(body: string, length = 160): string {
  const text = body
    .replace(/[\uE000-\uE004]/g, '')
    .replace(/!\[[^\]]*\]\([^)]*\)(\{[^}]*\})?/g, '')
    .replace(
      /\[\[([^\]|#]*)(#[^\]|]*)?(?:\|([^\]]*))?\]\]/g,
      (_all, target: string, _h, label?: string) => label ?? target,
    )
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/^\s*(?:>\s*)*(?:#+\s|[-*+]\s(?:\[[ xX]\]\s)?|\d+[.)]\s)?/gm, '')
    // tables read as their cells
    .replace(/^\s*\|?(\s*:?-+:?\s*\|)+\s*:?-*:?\s*$/gm, '')
    .replace(/\|/g, ' ')
    .replace(/[*_~`]/g, '')
  const flat = text.split(/\s+/).filter(Boolean).join(' ')
  return flat.length > length ? `${flat.slice(0, length)}…` : flat
}
