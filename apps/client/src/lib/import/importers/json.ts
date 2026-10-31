// SPDX-License-Identifier: AGPL-3.0-or-later
// Apps whose export is one json file: Simplenote (`notes.json`, alone or in
// its zip) and Standard Notes (a decrypted backup, alone or in its zip).

import { baseName, extension, readText } from '../source'
import type { Importer, ImportModel, ImportSource, ModelNote, Report } from '../types'

const time = (value: unknown) => {
  const parsed = typeof value === 'string' ? Date.parse(value) : NaN
  return Number.isFinite(parsed) ? parsed : undefined
}

async function readJson(source: ImportSource, path: string, report: Report): Promise<unknown> {
  try {
    return JSON.parse(await readText(source, path))
  } catch {
    report.add({ kind: 'unreadable', detail: path })
    return null
  }
}

/** Title is the first line, the rest the body, as Simplenote shows them. */
function firstLine(content: string): { title: string; body: string } {
  const text = content.replace(/\r\n?/g, '\n')
  const end = text.indexOf('\n')
  const title = (end < 0 ? text : text.slice(0, end)).replace(/^#+\s*/, '').trim()
  return { title, body: end < 0 ? '' : text.slice(end + 1).replace(/^\n+/, '') }
}

const simplenoteFile = (source: ImportSource) =>
  source.entries.find((e) => baseName(e.path) === 'notes.json')?.path

export const simplenoteImporter: Importer = {
  id: 'simplenote',
  app: 'Simplenote',
  picks: ['file'],
  extensions: ['zip', 'json'],
  detect: (source) => {
    const file = simplenoteFile(source)
    if (!file) return 0
    return /(^|\/)source\/notes\.json$/.test(file) ? 0.95 : 0.6
  },
  read: async (source, report) => {
    const file = simplenoteFile(source)
    const json = (file ? await readJson(source, file, report) : null) as {
      activeNotes?: Record<string, unknown>[]
      trashedNotes?: Record<string, unknown>[]
    } | null
    const notes: ModelNote[] = []
    const add = (item: Record<string, unknown>, trashed: boolean) => {
      const { title, body } = firstLine(String(item.content ?? ''))
      const system = Array.isArray(item.systemTags) ? item.systemTags : []
      notes.push({
        key: String(item.id ?? `${notes.length}`),
        folder: null,
        title,
        body,
        tags: Array.isArray(item.tags) ? item.tags.map(String) : [],
        created: time(item.creationDate),
        updated: time(item.lastModified),
        pinned: item.pinned === true || system.includes('pinned'),
        trashed,
        files: [],
      })
    }
    for (const item of json?.activeNotes ?? []) add(item, false)
    for (const item of json?.trashedNotes ?? []) add(item, true)
    return { folders: [], notes }
  },
}

interface LexicalNode {
  type?: string
  text?: string
  format?: number | string
  tag?: string
  listType?: string
  checked?: boolean
  url?: string
  language?: string
  children?: LexicalNode[]
}

/**
 * A Standard Notes "Super" note (a Lexical document) as markdown: the
 * common blocks and marks. `unknown` collects block types left as text.
 */
export function lexicalToMarkdown(doc: LexicalNode, unknown: Set<string>): string {
  const inline = (nodes: LexicalNode[] = []): string =>
    nodes
      .map((node) => {
        if (node.type === 'text' || node.type === 'code-highlight') {
          const format = typeof node.format === 'number' ? node.format : 0
          let text = node.text ?? ''
          if (format & 16) return `\`${text}\``
          if (format & 1) text = `**${text}**`
          if (format & 2) text = `*${text}*`
          if (format & 4) text = `~~${text}~~`
          return text
        }
        if (node.type === 'linebreak') return '\n'
        if (node.type === 'link' || node.type === 'autolink') {
          return `[${inline(node.children)}](${node.url ?? ''})`
        }
        return inline(node.children)
      })
      .join('')
  const block = (node: LexicalNode, depth = 0): string => {
    switch (node.type) {
      case 'paragraph':
        return inline(node.children)
      case 'heading':
        return `${'#'.repeat(Number(node.tag?.slice(1)) || 1)} ${inline(node.children)}`
      case 'quote':
        return `> ${inline(node.children)}`
      case 'code':
        return `\`\`\`${node.language ?? ''}\n${inline(node.children)}\n\`\`\``
      case 'horizontalrule':
        return '---'
      case 'list':
        return (node.children ?? [])
          .map((item, i) => {
            const nested = (item.children ?? []).filter((c) => c.type === 'list')
            const own = (item.children ?? []).filter((c) => c.type !== 'list')
            const marker =
              node.listType === 'number'
                ? `${i + 1}.`
                : node.listType === 'check'
                  ? `- [${item.checked ? 'x' : ' '}]`
                  : '-'
            const line = `${'  '.repeat(depth)}${marker} ${inline(own)}`
            return [line, ...nested.map((n) => block(n, depth + 1))].join('\n')
          })
          .join('\n')
      default:
        if (node.type) unknown.add(node.type)
        return inline(node.children)
    }
  }
  return (doc.children ?? [])
    .map((n) => block(n))
    .join('\n\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

const standardFile = (source: ImportSource) =>
  source.entries.find(
    (e) => /standard notes/i.test(baseName(e.path)) && ['txt', 'json'].includes(extension(e.path)),
  )?.path

interface SnItem {
  uuid?: string
  content_type?: string
  content?: Record<string, unknown> | string
  created_at?: string
  updated_at?: string
  deleted?: boolean
}

export const standardNotesImporter: Importer = {
  id: 'standard-notes',
  app: 'Standard Notes',
  picks: ['file'],
  extensions: ['txt', 'json', 'zip'],
  detect: (source) => (standardFile(source) ? 0.9 : 0),
  read: async (source, report): Promise<ImportModel> => {
    const file = standardFile(source)
    const json = (file ? await readJson(source, file, report) : null) as { items?: SnItem[] } | null
    const items = (json?.items ?? []).filter((i) => !i.deleted)
    // tags list the notes they hold
    const tagsOf = new Map<string, string[]>()
    for (const item of items) {
      if (item.content_type !== 'Tag' || typeof item.content !== 'object') continue
      const refs = (item.content.references ?? []) as { uuid?: string }[]
      for (const ref of refs) {
        if (ref.uuid)
          tagsOf.set(ref.uuid, [...(tagsOf.get(ref.uuid) ?? []), String(item.content.title ?? '')])
      }
    }
    const notes: ModelNote[] = []
    for (const item of items) {
      if (typeof item.content === 'string') {
        if (item.content_type === 'Note') report.add({ kind: 'encrypted', detail: item.uuid })
        continue
      }
      const content = item.content ?? {}
      if (item.content_type === 'SN|FileItem') {
        report.add({ kind: 'fileMissing', detail: String(content.name ?? item.uuid) })
        continue
      }
      if (item.content_type !== 'Note') continue
      const title = String(content.title ?? '')
      let body = String(content.text ?? '')
      if (content.noteType === 'super') {
        const unknown = new Set<string>()
        try {
          body = lexicalToMarkdown((JSON.parse(body) as { root?: LexicalNode }).root ?? {}, unknown)
        } catch {
          report.add({ kind: 'unreadable', note: title })
        }
        for (const type of unknown) report.add({ kind: 'simplified', note: title, detail: type })
      }
      const app = ((content.appData as Record<string, Record<string, unknown>> | undefined)?.[
        'org.standardnotes.sn'
      ] ?? {}) as Record<string, unknown>
      const tags = [...(tagsOf.get(item.uuid ?? '') ?? [])]
      if (app.archived === true) tags.push('archived')
      notes.push({
        key: item.uuid ?? `${notes.length}`,
        folder: null,
        title,
        body: body.replace(/\r\n?/g, '\n'),
        tags,
        created: time(item.created_at),
        updated: time(app.client_updated_at) ?? time(item.updated_at),
        pinned: app.pinned === true,
        trashed: content.trashed === true,
        files: [],
      })
    }
    return { folders: [], notes }
  },
}
