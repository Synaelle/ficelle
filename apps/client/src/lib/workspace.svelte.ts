// SPDX-License-Identifier: AGPL-3.0-or-later
// What the main screen shows: folders, tags, the note list for the current
// scope, search results and the open note. Components read these fields and
// call the methods, all storage goes through the backend.

import {
  backend,
  type BackendError,
  type Folder,
  type Id,
  type Note,
  type NoteChange,
  type NoteConflict,
  type Resolution,
  type NoteSummary,
  type Scope,
  type SearchHit,
  type Tag,
} from './backend'
import { isMarkdownFile, parseMarkdownFile } from './importing'
import { prefs } from './prefs.svelte'

/** Wait after the last keystroke before saving. A pause between words is
 * shorter, a pause to think is longer: one save per thought, not per word. */
const SAVE_DELAY_MS = 1000

export function isBackendError(err: unknown): err is BackendError {
  return typeof err === 'object' && err !== null && 'kind' in err
}

class Workspace {
  folders: Folder[] = $state([])
  tags: Tag[] = $state([])
  notes: NoteSummary[] = $state([])
  scope: Scope = $state({ kind: 'all' })
  /** Every attachment shows in place of the note list. */
  files = $state(false)
  query = $state('')
  hits: SearchHit[] | null = $state(null)
  current: Note | null = $state(null)
  /** What the open note waits for after a sync. */
  conflict: NoteConflict | null = $state(null)
  /** Notes waiting for a decision, marked in lists. */
  conflicted: Id[] = $state([])
  /** Last error to show, as a translation kind. */
  error: string | null = $state(null)
  /** True while an edit waits to be saved. */
  dirty = $state(false)
  /** Heading the next opened note should scroll to, from a [[Note#Heading]] link. */
  pendingHeading: string | null = null
  /** Grows when the open note's text is replaced (an older version brought
   * back), the editor then starts again from `current`. */
  revision = $state(0)

  private pending: NoteChange = {}
  /** The stored text the editor's text derives from. Sent with each save,
   * so an edit made elsewhere in the meantime is merged, not overwritten. */
  private base: { title: string; body: string } | null = null
  private timer: ReturnType<typeof setTimeout> | undefined

  async load() {
    await Promise.all([this.refreshFolders(), this.refreshTagsAndNotes(), this.refreshConflicts()])
  }

  async refreshTagsAndNotes() {
    await this.refreshTags()
    await this.refreshNotes()
  }

  async refreshConflicts() {
    this.conflicted = await backend.conflictedNotes()
    const id = this.current?.id
    this.conflict = id && this.conflicted.includes(id) ? await backend.noteConflict(id) : null
  }

  /** Settles the open note's conflict. Whatever is not kept stays in its history. */
  async resolve(keep: Resolution) {
    const note = this.current
    if (!note) return
    await this.flush()
    try {
      await backend.resolveConflict(note.id, keep)
      this.replace(await backend.getNote(note.id))
    } catch (err) {
      this.report(err)
    }
    await this.load()
  }

  async refreshFolders() {
    this.folders = await backend.listFolders()
  }

  async refreshTags() {
    this.tags = await backend.listTags()
    // a tag goes with its last note: viewing it, the list goes back to all notes
    const scope = this.scope
    if (scope.kind === 'tag' && !this.tags.some((tag) => tag.id === scope.id))
      this.scope = { kind: 'all' }
  }

  async refreshNotes() {
    this.notes = await backend.listNotes(this.scope, prefs.sort)
  }

  async setScope(scope: Scope) {
    await this.flush()
    this.files = false
    this.scope = scope
    this.query = ''
    this.hits = null
    await this.refreshNotes()
  }

  async search(query: string) {
    this.query = query
    this.hits = query.trim() ? await backend.search(query, 50) : null
  }

  async open(id: Id | null) {
    await this.leave(id)
    this.show(id ? await backend.getNote(id) : null)
    await this.refreshConflicts()
  }

  private show(note: Note | null) {
    if (this.conflict?.noteId !== note?.id) this.conflict = null
    this.current = note
    this.base = note ? { title: note.title, body: note.body } : null
  }

  /** Puts a new text of the open note in the editor, an older version
   * brought back for example. */
  replace(note: Note) {
    this.show(note)
    this.revision++
  }

  /** Whether a space is shared with this account to read only. The sharing
   * module sets it. */
  readOnly: (spaceId: string) => boolean = () => false

  /** The folder new notes go into: the one shown, unless it is shared to
   * read only, then none. */
  private newNoteFolder(): Id | null {
    if (this.scope.kind !== 'folder' || !this.scope.id) return null
    const id = this.scope.id
    const folder = this.folders.find((f) => f.id === id)
    return folder && this.readOnly(folder.spaceId) ? null : id
  }

  /**
   * Whether the open note follows other devices' edits as they come in:
   * the server allows co-editing and the person did not turn it off. The
   * sync module sets it.
   */
  coediting: () => boolean = () => false

  /** Another device's text for the open note, the editor patches it in. */
  private adopt(text: { title: string; body: string }) {
    if (!this.current) return
    this.current.title = text.title
    this.current.body = text.body
    this.base = { title: text.title, body: text.body }
  }

  /** Changes from another device were taken in. Lists follow. The open
   * note follows too when co-editing is on (an edit waiting is saved first,
   * the save merges both). With it off the open note stays as shown and
   * edits merge when saved. */
  async remoteChanged() {
    await this.load()
    const note = this.current
    if (!note) return
    if (this.coediting() && this.dirty) await this.flush()
    let fresh: Note | null
    try {
      fresh = await backend.getNote(note.id)
    } catch {
      fresh = null
    }
    if (this.current?.id !== note.id) return
    if (!fresh || fresh.trashedAt) {
      this.show(null)
      return
    }
    this.current.pinned = fresh.pinned
    this.current.tags = fresh.tags
    this.current.folderId = fresh.folderId
    const textChanged = fresh.title !== note.title || fresh.body !== note.body
    if (textChanged && this.coediting() && !this.dirty) this.adopt(fresh)
  }

  /**
   * Saves the open note before another takes its place. A note left with no
   * title and no text is not kept: creating one and backing out leaves
   * nothing behind.
   */
  private async leave(next: Id | null = null) {
    await this.flush()
    const note = this.current
    if (!note || note.id === next || note.title.trim() || note.body.trim() || note.pinned) return
    try {
      if (await backend.discardIfBlank(note.id)) await this.refreshNotes()
    } catch (err) {
      this.report(err)
    }
  }

  async openByTitle(title: string, heading: string | null = null) {
    const id = await backend.resolveLink(title)
    if (id) {
      this.pendingHeading = heading
      await this.open(id)
    }
    return id !== null
  }

  /**
   * Creates a note in the current folder or tag. With open false it is only
   * stored, for callers that finish it (attachments) before showing it: the
   * editor reads the body once, when the note opens.
   */
  async create(initial: { title?: string; body?: string } = {}, open = true) {
    if (open) await this.leave()
    const folderId = this.newNoteFolder()
    const tags = this.scope.kind === 'tag' ? [this.tagName(this.scope.id)] : []
    const note = await backend.createNote({ ...initial, folderId, tags })
    if (open) this.show(note)
    await this.refreshNotes()
    return note
  }

  /**
   * Creates a note from each markdown file, in the current folder or tag,
   * and opens the last one. Other files are skipped and reported.
   */
  async importFiles(files: File[]) {
    await this.leave()
    const markdown = files.filter(isMarkdownFile)
    if (markdown.length < files.length) this.error = 'notMarkdown'
    if (markdown.length === 0) return
    const folderId = this.newNoteFolder()
    const scopeTags = this.scope.kind === 'tag' ? [this.tagName(this.scope.id)] : []
    try {
      let last: Note | null = null
      for (const file of markdown) {
        const { title, body, tags } = parseMarkdownFile(file.name, await file.text())
        const allTags = [...new Set([...scopeTags, ...tags])]
        last = await backend.createNote({ title, body, folderId, tags: allTags })
      }
      this.show(last)
    } catch (err) {
      this.report(err)
    }
    await this.refreshTagsAndNotes()
  }

  /** Queues an edit of the open note and saves after a short pause. */
  edit(change: NoteChange) {
    if (!this.current) return
    Object.assign(this.current, change)
    Object.assign(this.pending, change)
    this.dirty = true
    clearTimeout(this.timer)
    this.timer = setTimeout(() => void this.flush(), SAVE_DELAY_MS)
  }

  /** The open editor's own save: it turns typing into markdown after a
   * short pause, so a flush asks it first (lock, close). */
  serializeEditor: (() => void) | null = null

  /** Saves a queued edit now. Called before leaving a note and on close. */
  async flush() {
    this.serializeEditor?.()
    clearTimeout(this.timer)
    const note = this.current
    const change = this.pending
    if (!note || Object.keys(change).length === 0) return
    this.pending = {}
    const base = this.base
    const sent: NoteChange = { ...change }
    if (base && change.title !== undefined) sent.baseTitle = base.title
    if (base && change.body !== undefined) sent.baseBody = base.body
    try {
      const saved = await backend.updateNote(note.id, sent)
      if (this.current?.id === saved.id) {
        const merged =
          (change.title !== undefined && saved.title !== change.title) ||
          (change.body !== undefined && saved.body !== change.body)
        const queued = Object.keys(this.pending).length > 0
        if (!queued) {
          this.current.updatedAt = saved.updatedAt
          this.dirty = false
        }
        if (!merged) this.base = { title: saved.title, body: saved.body }
        // another device's edit came in while this one was typed: with
        // co-editing the editor takes in the merge of both. Otherwise, or
        // with more typing queued, the editor still derives from the old
        // base and the next save merges again from it
        else if (!queued && this.coediting()) this.adopt(saved)
      }
      await Promise.all([this.refreshNotes(), this.refreshConflicts()])
      if (change.title !== undefined) await this.refreshTags()
    } catch (err) {
      this.report(err)
    }
  }

  async setTags(tags: string[]) {
    if (!this.current) return
    await backend.setNoteTags(this.current.id, tags)
    const fresh = await backend.getNote(this.current.id)
    if (this.current?.id === fresh.id) this.current.tags = fresh.tags
    await this.refreshTagsAndNotes()
  }

  async togglePin(id: Id, pinned: boolean) {
    await backend.setPinned(id, pinned)
    if (this.current?.id === id) this.current.pinned = pinned
    await this.refreshNotes()
  }

  async trash(id: Id) {
    if (this.current?.id === id) {
      await this.flush()
      this.current = null
    }
    await backend.trashNote(id)
    await this.refreshTagsAndNotes()
  }

  async restore(id: Id) {
    await backend.restoreNote(id)
    await this.refreshTagsAndNotes()
  }

  async purge(id: Id) {
    if (this.current?.id === id) this.current = null
    await backend.purgeNote(id)
    await this.refreshNotes()
  }

  async move(id: Id, folder: Id | null) {
    const moved = await backend.moveNote(id, folder)
    // into or out of a shared folder, the note is a copy with a new id
    if (this.current?.id === id) {
      if (moved === id) this.current.folderId = folder
      else await this.open(moved)
    }
    await this.refreshNotes()
  }

  async createFolder(parent: Id | null, name: string) {
    const folder = await backend.createFolder(parent, name)
    await this.refreshFolders()
    return folder
  }

  async renameFolder(id: Id, name: string) {
    await backend.renameFolder(id, name)
    await this.refreshFolders()
  }

  async trashFolder(id: Id) {
    await backend.trashFolder(id)
    if (this.scope.kind === 'folder' && this.scope.id === id) this.scope = { kind: 'all' }
    await this.load()
  }

  tagName(id: Id): string {
    return this.tags.find((t) => t.id === id)?.name ?? ''
  }

  folderName(id: Id | null): string {
    return this.folders.find((f) => f.id === id)?.name ?? ''
  }

  report(err: unknown) {
    this.error = isBackendError(err) ? err.kind : 'unknown'
    console.error(err)
  }
}

export const workspace = new Workspace()
