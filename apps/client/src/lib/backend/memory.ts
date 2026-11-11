// SPDX-License-Identifier: AGPL-3.0-or-later
// In-memory backend for the browser preview (pnpm dev:web) and ui tests.
// It follows the core rules closely enough to exercise the ui, the real
// rules and their tests live in crates/core.

import type { Backend } from './backend'
import { sampleData } from './sample'
import {
  MATCH_END,
  MATCH_START,
  type Attachment,
  type Folder,
  type Id,
  type Note,
  type NoteChange,
  type NoteSummary,
  type NoteVersion,
  type Scope,
  type Tag,
  type TagColor,
} from './types'

/** The memory backend, with what tests read back. */
export type MemoryBackend = Backend & {
  /** Feedback reports "sent", for tests. */
  sentFeedback: import('./types').FeedbackDraft[]
}

const key = (title: string) => title.trim().toLowerCase()
/** A tag as the app keeps it: levels joined by `/`, each without `#`. */
const cleanTag = (name: string) =>
  name
    .split('/')
    .map((p) => p.trim().replace(/^#+/, '').split(/\s+/).filter(Boolean).join(' '))
    .filter(Boolean)
    .join('/')
/** `name` is `under` or a tag it holds. */
const within = (name: string, under: string) =>
  key(name) === key(under) || key(name).startsWith(`${key(under)}/`)
const linkTargets = (body: string) =>
  [...body.replace(/```[\s\S]*?```|`[^`\n]*`/g, '').matchAll(/\[\[([^\]\n|#]+)/g)].map((m) =>
    key(m[1] ?? ''),
  )

/** Rough version of crates/core/src/preview.rs, good enough for the preview:
 * no code, and no first line repeating the title. */
function preview(title: string, body: string): string {
  const lines = body.replace(/^```[^\n]*\n[\s\S]*?^```.*$/gm, '').split('\n')
  const first = lines.findIndex((line) => line.trim())
  const repeats = (line: string) =>
    line
      .replace(/^#+\s*/, '')
      .trim()
      .toLowerCase() === title.trim().toLowerCase()
  if (title.trim() && first >= 0 && repeats(lines[first] ?? '')) lines.splice(first, 1)
  const plain = lines
    .join('\n')
    .replace(/!\[[^\]]*\]\([^)]*\)(\{[^}]*\})?/g, '')
    .replace(
      /\[\[([^\]|#]+)(#[^\]|]*)?(\|([^\]]+))?\]\]/g,
      (_m, target: string, _h, _p, label?: string) => label ?? target,
    )
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/^\s*(>\s*)*(#+\s|[-*+]\s(\[[ xX]\]\s)?|\d+[.)]\s|\[![\w-]+\]\s?)?/gm, '')
    .replace(/[*~`]/g, '')
  // the first line with text, as the list shows only that
  const line = plain.split('\n').find((l) => l.trim()) ?? ''
  const flat = line.split(/\s+/).filter(Boolean).join(' ')
  return flat.length > 140 ? `${flat.slice(0, 140)}…` : flat
}

function summary(note: Note): NoteSummary {
  const { id, folderId, title, body, pinned, updatedAt, createdAt, spaceId, tags } = note
  return {
    tags: [...tags],
    id,
    folderId,
    title,
    snippet: preview(title, body),
    pinned,
    updatedAt,
    createdAt,
    spaceId,
  }
}

export function createMemoryBackend(options: { seed?: boolean } = {}): MemoryBackend {
  let unlocked = false
  let exists = false
  let encrypted = false
  const notes = new Map<Id, Note>()
  const folders = new Map<Id, Folder>()
  /** Tags made on their own and colour marks, by lowercased path. */
  const tagMarks = new Map<string, { name: string; color: TagColor | null }>()
  const versions: NoteVersion[] = []
  const sentFeedback: import('./types').FeedbackDraft[] = []
  const attachments = new Map<Id, Attachment & { url: string }>()
  const now = () => Date.now()
  const id = () => crypto.randomUUID()
  // imports: where each note came from, files stored before a batch, the
  // folders of the import being written
  const imported = new Map<string, Id>()
  const stored = new Map<string, Uint8Array>()
  let importRoot: Id | null = null
  const importFolders = new Map<string, Id>()

  const note = (noteId: Id) => {
    const found = notes.get(noteId)
    if (!found) throw { kind: 'notFound', message: 'note not found' }
    return found
  }

  if (options.seed) {
    exists = true
    unlocked = true
    sampleData(now()).notes.forEach((n) => notes.set(n.id, n))
  }

  // backups: a folder whose path holds "unreachable" fails, as an unplugged drive
  const backups: import('./types').BackupsView = {
    config: null,
    status: {
      lastRunAt: null,
      lastSuccessAt: null,
      lastError: null,
      lastErrorKind: null,
      lastReport: null,
      snapshots: 0,
      storedBytes: 0,
    },
    running: false,
    kinds: ['folder', 's3', 'webdav'],
  }
  const backupSnapshots: import('./types').BackupSnapshot[] = []
  /** Backups already in a folder, by path: a path holding "old-backups"
   * has two with the password "old password", "open-backups" two made
   * without one (to see the restore steps in the browser). */
  const repos = new Map<string, string>()
  const seeded = (path: string): string | null =>
    repos.get(path) ??
    (path.includes('old-backups') ? 'old password' : path.includes('open-backups') ? '' : null)
  const oldSnapshots = (host: string): import('./types').BackupSnapshot[] =>
    [3, 30].map((days) => ({
      id: id(),
      time: now() - days * 86_400_000,
      host,
      appVersion: 'test',
      counts: { notes: notes.size + days, folders: folders.size, versions: 0, files: 2 },
      size: 0,
    }))
  const backend: MemoryBackend = {
    native: false,
    vaultStatus: async () => ({
      exists,
      encrypted,
      unlocked,
      way: !exists || encrypted ? 'password' : 'plain',
      hasPassword: encrypted,
      biometricAvailable: false,
      deviceAvailable: false,
      recovery: false,
      pinTriesLeft: null,
    }),
    vaultCreate: async (way) => {
      exists = true
      encrypted = way !== 'plain'
      unlocked = true
    },
    vaultUnlock: async (password) => {
      if (encrypted && password === null) throw { kind: 'notRemembered', message: '' }
      unlocked = true
    },
    vaultUnlockPin: async () => {
      throw { kind: 'unsupported', message: 'a PIN needs the app' }
    },
    vaultUnlockBiometric: async () => {
      throw { kind: 'unsupported', message: 'biometric unlock needs the app' }
    },
    vaultSetWay: async () => {
      throw { kind: 'unsupported', message: 'unlock ways need the app' }
    },
    vaultEncrypt: async () => {
      throw { kind: 'unsupported', message: 'encryption needs the app' }
    },
    appUpdateCheck: async () => null,
    appUpdateLatest: async () => null,
    notificationsAllowed: async () => false,
    notify: async () => {},
    // the browser fetches the font itself, nothing is kept
    fontDownload: async (_id, files) => files ?? ['latin-400-normal', 'latin-700-normal'],
    fontFiles: async () => [],
    fontFile: async (id, name) => {
      const response = await fetch(
        `https://cdn.jsdelivr.net/fontsource/fonts/${id}@latest/${name}.woff2`,
      )
      if (!response.ok) throw { kind: 'notFound', message: `${id} has no ${name}` }
      return response.arrayBuffer()
    },
    systemClock24h: async () => null,
    // the page's own paste event is all the browser has
    clipboardImages: async () => [],
    clipboardImage: async () => new ArrayBuffer(0),
    droppedImages: async () => [],
    droppedImage: async () => new ArrayBuffer(0),
    appUpdatePageOnly: async () => false,
    appUpdateInstall: async () => {
      throw { kind: 'unsupported', message: 'updates need the app' }
    },
    quickWindowOpen: async () => {
      throw { kind: 'unsupported', message: 'the quick note needs the desktop app' }
    },
    quickWindowReady: async () => {},
    vaultCheckPassword: async (password) => password.length > 0,
    vaultCreateRecovery: async () => {
      throw { kind: 'unsupported', message: 'recovery keys need the app' }
    },
    vaultRemoveRecovery: async () => {
      throw { kind: 'unsupported', message: 'recovery keys need the app' }
    },
    vaultRecover: async () => {
      throw { kind: 'unsupported', message: 'recovery keys need the app' }
    },
    vaultLock: async () => {
      unlocked = false
    },
    vaultReset: async () => {
      unlocked = false
      exists = false
      encrypted = false
      notes.clear()
      folders.clear()
    },
    vaultChangePassword: async () => {},

    listNotes: async (scope: Scope, sort) => {
      const tagName = scope.kind === 'tag' ? scope.id : null
      if (scope.kind === 'untagged')
        return [...notes.values()]
          .filter((n) => n.trashedAt === null && n.tags.length === 0)
          .map(summary)
      const list = [...notes.values()].filter((n) => {
        if (scope.kind === 'trash') return n.trashedAt !== null
        if (n.trashedAt !== null) return false
        if (scope.kind === 'folder') return n.folderId === scope.id
        if (tagName) return n.tags.some((t) => within(t, tagName))
        return true
      })
      const order: Record<typeof sort, (a: Note, b: Note) => number> = {
        updatedDesc: (a, b) => b.updatedAt - a.updatedAt,
        createdDesc: (a, b) => b.createdAt - a.createdAt,
        titleAsc: (a, b) => a.title.localeCompare(b.title),
      }
      list.sort((a, b) => Number(b.pinned) - Number(a.pinned) || order[sort](a, b))
      return list.map(summary)
    },
    getNote: async (noteId) => structuredClone(note(noteId)),
    createNote: async (input) => {
      const created: Note = {
        id: id(),
        folderId: input.folderId ?? null,
        title: input.title ?? '',
        body: input.body ?? '',
        pinned: input.pinned ?? false,
        tags: input.tags ?? [],
        createdAt: now(),
        updatedAt: now(),
        trashedAt: null,
        spaceId: '',
      }
      notes.set(created.id, created)
      return structuredClone(created)
    },
    updateNote: async (noteId, change) => {
      const current = note(noteId)
      if (current.trashedAt !== null && !change.inTrash)
        throw { kind: 'invalid', message: 'restore the note from the trash before editing it' }
      versions.unshift({
        id: id(),
        noteId,
        title: current.title,
        body: current.body,
        createdAt: now(),
        reason: 'edit',
      })
      // a title cleared to type another one: links follow from the one before
      const oldTitle =
        current.title.trim() ||
        (versions.find((v) => v.noteId === noteId && v.title.trim())?.title ?? '')
      // no other device here, the base of an edit never differs
      const edit: NoteChange = { ...change }
      delete edit.baseTitle
      delete edit.baseBody
      delete edit.inTrash
      Object.assign(current, edit, { updatedAt: now() })
      if (
        change.title !== undefined &&
        key(change.title) &&
        key(oldTitle) &&
        key(oldTitle) !== key(change.title)
      ) {
        for (const other of notes.values()) {
          if (other.id === noteId) continue
          other.body = other.body.replace(/\[\[([^\]\n|#]+)/g, (all, target: string) =>
            key(target) === key(oldTitle) ? `[[${change.title}` : all,
          )
        }
      }
      return structuredClone(current)
    },
    setPinned: async (noteId, pinned) => {
      note(noteId).pinned = pinned
    },
    moveNote: async (noteId, folder) => {
      note(noteId).folderId = folder
      return noteId
    },
    setNoteTags: async (noteId, tags) => {
      const seen = new Map<string, string>()
      for (const t of tags.map(cleanTag).filter(Boolean)) if (!seen.has(key(t))) seen.set(key(t), t)
      note(noteId).tags = [...seen.values()]
    },
    trashNote: async (noteId) => {
      note(noteId).trashedAt = now()
    },
    restoreNote: async (noteId) => {
      note(noteId).trashedAt = null
    },
    purgeNote: async (noteId) => {
      notes.delete(noteId)
    },
    discardIfBlank: async (noteId) => {
      const note = notes.get(noteId)
      const blank =
        !!note && !note.trashedAt && !note.pinned && !note.title.trim() && !note.body.trim()
      // an emptied note goes to the trash, as in the app
      const held = versions.some((v) => v.noteId === noteId && (v.title.trim() || v.body.trim()))
      if (blank && held) note.trashedAt = now()
      else if (blank) notes.delete(noteId)
      return blank
    },
    emptyTrash: async () => {
      const gone = [...notes.values()].filter((n) => n.trashedAt !== null)
      gone.forEach((n) => notes.delete(n.id))
      for (const f of [...folders.values()]) if (f.trashedAt !== null) folders.delete(f.id)
      return gone.length
    },

    listFolders: async () => [...folders.values()].filter((f) => f.trashedAt === null),
    createFolder: async (parent, name) => {
      const folder: Folder = {
        id: id(),
        parentId: parent,
        name: name.trim(),
        position: [...folders.values()].filter((f) => f.parentId === parent).length,
        createdAt: now(),
        updatedAt: now(),
        trashedAt: null,
        spaceId: '',
      }
      folders.set(folder.id, folder)
      return folder
    },
    renameFolder: async (folderId, name) => {
      const folder = folders.get(folderId)
      if (folder) folder.name = name.trim()
    },
    moveFolder: async (folderId, parent) => {
      const folder = folders.get(folderId)
      if (folder) folder.parentId = parent
    },
    trashFolder: async (folderId) => {
      const when = now()
      const folder = folders.get(folderId)
      if (folder) folder.trashedAt = when
      for (const n of notes.values()) {
        if (n.folderId === folderId && n.trashedAt === null) n.trashedAt = when
      }
    },
    listTrashedFolders: async () =>
      [...folders.values()].filter((f) => {
        if (f.trashedAt === null) return false
        const parent = f.parentId ? folders.get(f.parentId) : undefined
        return parent?.trashedAt !== f.trashedAt
      }),
    restoreFolder: async (folderId) => {
      const folder = folders.get(folderId)
      if (!folder?.trashedAt) return
      for (const n of notes.values()) {
        if (n.folderId === folderId && n.trashedAt === folder.trashedAt) n.trashedAt = null
      }
      folder.trashedAt = null
    },

    listTags: async () => {
      const names = new Map<string, string>()
      const add = (name: string) => {
        const levels = name.split('/')
        levels.forEach((_, i) => {
          const level = levels.slice(0, i + 1).join('/')
          if (!names.has(key(level))) names.set(key(level), level)
        })
      }
      for (const n of notes.values()) n.tags.forEach(add)
      for (const mark of tagMarks.values()) add(mark.name)
      const live = [...notes.values()].filter((n) => n.trashedAt === null)
      return [...names.values()]
        .map((name): Tag => ({
          id: name,
          name,
          noteCount: live.filter((n) => n.tags.some((t) => within(t, name))).length,
          color: tagMarks.get(key(name))?.color ?? null,
        }))
        .sort((a, b) => a.name.localeCompare(b.name))
    },
    createTag: async (name) => {
      const clean = cleanTag(name)
      if (!clean) throw { kind: 'invalid', message: 'a tag needs a name' }
      const mark = tagMarks.get(key(clean))
      tagMarks.set(key(clean), { name: mark?.name ?? clean, color: mark?.color ?? null })
      return clean
    },
    renameTag: async (tagId, name) => {
      const clean = cleanTag(name)
      if (!clean) throw { kind: 'invalid', message: 'a tag needs a name' }
      if (key(clean) !== key(tagId) && within(clean, tagId))
        throw { kind: 'invalid', message: 'a tag cannot go inside itself' }
      const moved = (t: string) => (within(t, tagId) ? clean + t.slice(tagId.length) : t)
      for (const n of notes.values()) {
        const seen = new Map<string, string>()
        for (const t of n.tags.map(moved)) if (!seen.has(key(t))) seen.set(key(t), t)
        n.tags = [...seen.values()]
      }
      for (const [k, mark] of [...tagMarks]) {
        if (!within(mark.name, tagId)) continue
        tagMarks.delete(k)
        tagMarks.set(key(moved(mark.name)), { ...mark, name: moved(mark.name) })
      }
    },
    setTagColor: async (tagId, color) => {
      const mark = tagMarks.get(key(tagId))
      if (color) tagMarks.set(key(tagId), { name: mark?.name ?? tagId, color })
      else if (mark) tagMarks.set(key(tagId), { ...mark, color: null })
    },
    deleteTag: async (tagId) => {
      for (const n of notes.values()) n.tags = n.tags.filter((t) => !within(t, tagId))
      for (const [k, mark] of [...tagMarks]) if (within(mark.name, tagId)) tagMarks.delete(k)
    },
    tagsMerged: async () => false,
    tagsMergedSeen: async () => {},
    listVersions: async (noteId) => versions.filter((v) => v.noteId === noteId),
    restoreVersion: async (versionId) => {
      const version = versions.find((v) => v.id === versionId)
      if (!version) throw { kind: 'notFound', message: 'version not found' }
      return backend.updateNote(version.noteId, { title: version.title, body: version.body })
    },
    // one device, nothing to merge
    noteConflict: async () => null,
    conflictedNotes: async () => [],
    resolveConflict: async () => {
      throw { kind: 'notFound', message: 'conflict not found' }
    },

    search: async (query, limit) => {
      const words = query.toLowerCase().split(/\s+/).filter(Boolean)
      if (words.length === 0) return []
      return [...notes.values()]
        .filter((n) => n.trashedAt === null)
        .filter((n) =>
          words.every((w) => `${n.title} ${n.body} ${n.tags.join(' ')}`.toLowerCase().includes(w)),
        )
        .slice(0, limit)
        .map((n) => {
          const first = words[0] ?? ''
          const at = Math.max(0, n.body.toLowerCase().indexOf(first) - 40)
          const part = n.body.slice(at, at + 120)
          const marked = part.replace(
            new RegExp(first.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'ig'),
            (m) => `${MATCH_START}${m}${MATCH_END}`,
          )
          return { noteId: n.id, title: n.title, snippet: marked }
        })
    },
    backlinks: async (noteId) => {
      const target = key(note(noteId).title)
      return [...notes.values()]
        .filter(
          (n) => n.id !== noteId && n.trashedAt === null && linkTargets(n.body).includes(target),
        )
        .map(summary)
    },
    resolveLink: async (target) =>
      [...notes.values()].find((n) => n.trashedAt === null && key(n.title) === key(target))?.id ??
      null,
    linkSuggestions: async (input, limit) =>
      [...notes.values()]
        .filter((n) => n.trashedAt === null && n.title && key(n.title).includes(key(input)))
        .sort(
          (a, b) =>
            Number(key(b.title).startsWith(key(input))) -
            Number(key(a.title).startsWith(key(input))),
        )
        .slice(0, limit)
        .map((n) => ({ id: n.id, title: n.title })),
    noteHeadings: async (noteId) =>
      [...note(noteId).body.matchAll(/^#{1,6} (.+)$/gm)].map((m) => (m[1] ?? '').trim()),

    attachInboxFile: async () => {
      throw { kind: 'unsupported', message: 'no inbox in the browser' }
    },
    addAttachment: async (noteId, file) => {
      const taken = new Set(
        [...attachments.values()].filter((a) => a.noteId === noteId).map((a) => a.fileName),
      )
      let name = file.name
      for (let n = 2; taken.has(name); n += 1) name = file.name.replace(/(\.[^.]*)?$/, ` ${n}$1`)
      const attachment = {
        id: id(),
        noteId,
        fileName: name,
        mimeType: file.type,
        size: file.size,
        sha256: id().replace(/-/g, '').padEnd(64, '0'),
        createdAt: now(),
        thumbSha256: null,
        url: URL.createObjectURL(file),
      }
      attachments.set(attachment.id, attachment)
      return attachment
    },
    listAttachments: async (noteId) => [...attachments.values()].filter((a) => a.noteId === noteId),
    allAttachments: async () =>
      [...attachments.values()]
        .filter((a) => notes.get(a.noteId) && !notes.get(a.noteId)?.trashedAt)
        .map((a) => ({
          id: a.id,
          noteId: a.noteId,
          fileName: a.fileName,
          mimeType: a.mimeType,
          size: a.size,
          sha256: a.sha256,
          createdAt: a.createdAt,
          thumbSha256: a.thumbSha256,
          noteTitle: notes.get(a.noteId)?.title ?? '',
        }))
        .reverse(),
    removeAttachment: async (attachmentId) => {
      attachments.delete(attachmentId)
    },
    fileUrl: (sha256) => [...attachments.values()].find((a) => a.sha256 === sha256)?.url ?? '',
    attachmentFiles: async (noteId) =>
      [...attachments.values()]
        .filter((a) => a.noteId === noteId)
        .map((a) => ({ ...a, here: true, refused: null })),
    fetchAttachment: async () => {},
    saveAttachment: async () => {},
    readAttachment: async () => {
      throw { kind: 'notFound', message: 'no files in the browser test backend' }
    },
    setAttachmentThumb: async () => {},
    filesInfo: async () => null,
    setAutoFetch: async () => {},
    exportMarkdown: async () => {
      throw { kind: 'unsupported', message: 'export needs the desktop app' }
    },

    exportMarkdownZip: async () => {
      throw { kind: 'unsupported', message: 'export needs the desktop app' }
    },
    backupDefaultName: async () => 'backup',
    backupWrite: async () => {
      throw { kind: 'unsupported', message: 'backups need the app' }
    },
    backupEncrypted: async () => false,
    backupInspect: async () => {
      throw { kind: 'unsupported', message: 'backups need the app' }
    },
    backupRestore: async () => {
      throw { kind: 'unsupported', message: 'backups need the app' }
    },

    importOpen: async () => {
      throw { kind: 'unsupported', message: 'opening an export needs the app' }
    },
    importRead: async () => {
      throw { kind: 'unsupported', message: 'opening an export needs the app' }
    },
    importKnown: async (importer, keys) =>
      keys.filter((k) => {
        const noteId = imported.get(`${importer}
${k}`)
        return noteId !== undefined && notes.has(noteId)
      }),
    importBegin: async (_source, name) => {
      importRoot = (await backend.createFolder(null, name)).id
      importFolders.clear()
      return importRoot
    },
    importPut: async (bytes) => {
      const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes.slice()))
      const sha256 = [...digest].map((b) => b.toString(16).padStart(2, '0')).join('')
      stored.set(sha256, bytes)
      return { sha256, size: bytes.length }
    },
    importWrite: async (_source, batch) => {
      const root = importRoot
      if (!root) throw { kind: 'invalid', message: 'the import has no folder yet' }
      const place = (k: string | null) => (k !== null ? importFolders.get(k) : undefined) ?? root
      for (const f of batch.folders) {
        if (!importFolders.has(f.key)) {
          importFolders.set(
            f.key,
            (await backend.createFolder(place(f.parent), f.name || 'Folder')).id,
          )
        }
      }
      const written: [string, Id][] = []
      const leftOut: { note: string; name: string; why: string }[] = []
      for (const input of batch.notes) {
        const created = input.created ?? input.updated ?? now()
        const made: Note = {
          id: id(),
          folderId: place(input.folder),
          title: input.title,
          body: input.body,
          pinned: input.pinned,
          tags: [...new Set(input.tags)],
          createdAt: created,
          updatedAt: Math.max(input.updated ?? created, created),
          trashedAt: input.trashed ? now() : null,
          spaceId: '',
        }
        notes.set(made.id, made)
        for (const file of input.sources) {
          const bytes = file.sha256 ? stored.get(file.sha256) : undefined
          if (!bytes || !file.sha256) {
            leftOut.push({ note: input.title, name: file.name, why: 'unreadable' })
            continue
          }
          const attachment = {
            id: id(),
            noteId: made.id,
            fileName: file.name,
            mimeType: file.mime,
            size: bytes.length,
            sha256: file.sha256,
            createdAt: created,
            thumbSha256: null,
            url: URL.createObjectURL(new Blob([bytes.slice()], { type: file.mime })),
          }
          attachments.set(attachment.id, attachment)
        }
        if (input.key)
          imported.set(
            `${batch.importer}
${input.key}`,
            made.id,
          )
        written.push([input.key, made.id])
      }
      return { notes: written, leftOut }
    },
    importClose: async () => {},

    sentFeedback,
    backupsGet: async () => structuredClone(backups),
    backupsFound: async (target) => {
      const there = target.kind === 'folder' ? seeded(target.path) : null
      return { found: there !== null, needsPassword: !!there }
    },
    backupsPickFolder: async () => null,
    backupsConnect: async (target, password, create, host) => {
      if (target.kind === 'folder') {
        const there = seeded(target.path)
        if (create && there !== null)
          throw { kind: 'invalid', message: 'backups are already there' }
        if (!create && there === null) throw { kind: 'notFound', message: 'no backups there' }
        if (!create && there !== password)
          throw { kind: 'wrongPassword', message: 'wrong password' }
        if (!create && !repos.has(target.path)) {
          // backups made elsewhere: theirs to restore from
          repos.set(target.path, password)
          backupSnapshots.length = 0
          backupSnapshots.push(...oldSnapshots('old laptop'))
        }
        if (create) repos.set(target.path, password)
      }
      backups.config = {
        target,
        schedule: backups.config?.schedule ?? 'daily',
        policy: backups.config?.policy ?? { last: 10, daily: 7, weekly: 4, monthly: 12, yearly: 0 },
        host: host || 'this device',
      }
      if (create) backupSnapshots.length = 0
      return structuredClone(backups)
    },
    backupsSettings: async (schedule, policy) => {
      if (!backups.config) throw { kind: 'notFound', message: 'backups are not set up' }
      backups.config = { ...backups.config, schedule, policy }
      return structuredClone(backups)
    },
    backupsDisconnect: async () => {
      backups.config = null
      return structuredClone(backups)
    },
    backupsRun: async () => {
      const config = backups.config
      if (!config) throw { kind: 'notFound', message: 'backups are not set up' }
      backups.status.lastRunAt = now()
      if (config.target.kind === 'folder' && config.target.path.includes('unreachable')) {
        backups.status.lastError = 'the folder could not be reached'
        backups.status.lastErrorKind = 'notFound'
        throw { kind: 'target', message: 'the folder could not be reached' }
      }
      const snapshot = id()
      backupSnapshots.unshift({
        id: snapshot,
        time: now(),
        host: config.host,
        appVersion: 'test',
        counts: { notes: notes.size, folders: folders.size, versions: 0, files: 0 },
        size: 0,
      })
      backups.status = {
        ...backups.status,
        lastSuccessAt: now(),
        lastError: null,
        snapshots: backupSnapshots.length,
      }
      return structuredClone(backups)
    },
    backupsSnapshots: async () => structuredClone(backupSnapshots),
    backupsRestore: async () => ({
      notes: 0,
      folders: 0,
      versions: 0,
      files: 0,
      skipped: notes.size,
      filesMissing: 0,
    }),
    backupsTestRestore: async (snapshot) => ({
      snapshot,
      restored: {
        notes: notes.size,
        folders: folders.size,
        versions: 0,
        files: 0,
        skipped: 0,
        filesMissing: 0,
      },
      problems: [],
    }),
    backupsCheck: async () => ({
      snapshots: backupSnapshots.length,
      packs: 1,
      objects: 1,
      storedBytes: 0,
      problems: [],
    }),
    feedbackWhere: async () => ({
      available: true,
      url: 'https://feedback.example.org',
      host: 'feedback.example.org',
      via: 'main',
      version: 'dev',
      platform: 'web',
    }),
    feedbackSend: async (draft) => {
      sentFeedback.push(draft)
      return {
        host: 'feedback.example.org',
        url: 'https://feedback.example.org',
        bits: 18,
        workMs: 1,
      }
    },
    feedbackAnswers: async (_url, secrets) => ({
      reports: secrets.map((secret) => ({ secret, state: 'received' as const, answer: '' })),
    }),
  }
  return backend
}
