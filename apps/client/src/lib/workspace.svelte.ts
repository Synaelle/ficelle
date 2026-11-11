// SPDX-License-Identifier: AGPL-3.0-or-later
// What the main screen shows: tags, the note list for the current
// scope, search results and the open note. Components read these fields and
// call the methods, all storage goes through the backend.

import {
  backend,
  type BackendError,
  type Id,
  type Note,
  type NoteChange,
  type NoteConflict,
  type Resolution,
  type NoteSummary,
  type Scope,
  type SearchHit,
  type Tag,
  type TagColor,
} from './backend'
import { t } from './i18n.svelte'
import { isMarkdownFile, parseMarkdownFile } from './importing'
import { prefs } from './prefs.svelte'
import { toasts } from './toast.svelte'

/** The notes with no tag: the view the app opens on, and the one it goes
 * back to when a filter is turned off or goes away. */
export const UNFILED: Scope = { kind: 'untagged' }

/** What makes two tag names one tag. */
export const tagKey = (name: string) => name.toLowerCase()

/** How many tags hold this one. */
const tagDepth = (name: string) => name.split('/').length - 1

/** `name` is `under` or one of the tags it holds. */
export const tagWithin = (name: string, under: string) =>
  tagKey(name) === tagKey(under) || tagKey(name).startsWith(`${tagKey(under)}/`)

/** Moves a dragged tag inside another, or to the top with `into` empty. A
 * tag dropped on itself or inside itself stays where it is. */
export function moveTag(name: string, into: string) {
  const tag = workspace.tags.find((t) => tagKey(t.name) === tagKey(name))
  if (!tag || (into && tagWithin(into, name))) return
  const leaf = name.slice(name.lastIndexOf('/') + 1)
  const target = into ? `${into}/${leaf}` : leaf
  if (tagKey(target) === tagKey(name)) return
  void workspace.renameTag(tag.id, target).catch((err) => workspace.report(err))
}

/** Wait after the last keystroke before saving. A pause between words is
 * shorter, a pause to think is longer: one save per thought, not per word. */
const SAVE_DELAY_MS = 1000

/** Typing with no such pause is saved this often all the same: the other
 * devices follow it while it goes on, and less is lost if the app stops. */
const SAVE_AT_MOST_MS = 2500

/** Wait before trying a save that failed again. */
const RETRY_DELAY_MS = 5000

export function isBackendError(err: unknown): err is BackendError {
  return typeof err === 'object' && err !== null && 'kind' in err
}

class Workspace {
  tags: Tag[] = $state([])
  notes: NoteSummary[] = $state([])
  scope: Scope = $state(UNFILED)
  /** Every attachment shows in place of the note list. */
  files = $state(false)
  query = $state('')
  hits: SearchHit[] | null = $state(null)
  current: Note | null = $state(null)
  /** A note just made here, its title takes the caret once shown. */
  fresh: Id | null = $state(null)
  /** What the open note waits for after a sync. */
  conflict: NoteConflict | null = $state(null)
  /** Notes waiting for a decision, marked in lists. */
  conflicted: Id[] = $state([])
  /** Last error to show, as a translation kind. */
  error: string | null = $state(null)
  /** True while an edit waits to be saved. */
  dirty = $state(false)
  /** The last save failed: the edit waits, tried again, and the note stays
   * open until it is saved. */
  unsaved = $state(false)
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
  /** When the edit waiting to be saved began. */
  private editedSince: number | null = null
  /** The save on its way to the store, one at a time. */
  private saving: Promise<void> | null = null
  /** Changes from another device are being taken in, and more came since. */
  private following = false
  private followAgain = false

  async load() {
    await Promise.all([this.refreshTagsAndNotes(), this.refreshConflicts()])
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

  async refreshTags() {
    this.tags = await backend.listTags()
    // a tag goes with its last note: viewing it, the list goes back to the
    // notes with no tag
    const scope = this.scope
    if (scope.kind === 'tag' && !this.tags.some((tag) => tag.id === scope.id)) this.scope = UNFILED
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

  /** A search result picked: the list moves to where the note lives, its most
   * nested tag, or the notes with no tag. */
  async openFound(id: Id) {
    if (!(await this.leave(id))) return
    const note = await backend.getNote(id)
    const deepest = note.tags
      .map((name) => this.tags.find((tag) => tagKey(tag.name) === tagKey(name)))
      .filter((tag) => tag !== undefined)
      .reduce<Tag | undefined>(
        (best, tag) => (!best || tagDepth(tag.name) > tagDepth(best.name) ? tag : best),
        undefined,
      )
    await this.setScope(deepest ? { kind: 'tag', id: deepest.id } : UNFILED)
    this.show(note)
    await this.refreshConflicts()
  }

  async open(id: Id | null) {
    if (!(await this.leave(id))) return
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

  /**
   * Whether the open note follows other devices' edits as they come in:
   * always between the person's own paired devices, and with a server when
   * it allows co-editing and the person did not turn it off. The sync
   * module sets it.
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
   * edits merge when saved. Changes arriving while this runs are taken in
   * right after, in order, never two at once. */
  async remoteChanged() {
    if (this.following) {
      this.followAgain = true
      return
    }
    this.following = true
    try {
      do {
        this.followAgain = false
        await this.follow()
      } while (this.followAgain)
    } finally {
      this.following = false
    }
  }

  /**
   * The tag being viewed is gone after another device's changes: where its
   * notes went, when the tag or one holding it was renamed or moved there.
   * Read from a note that was in the list, by the tag it has now in place
   * of the one it had.
   */
  private async renamedTag(viewed: string, listed: NoteSummary[], known: Set<string>) {
    for (const was of listed.slice(0, 20)) {
      let note: Note
      try {
        note = await backend.getNote(was.id)
      } catch {
        continue
      }
      for (const old of was.tags.filter((name) => tagWithin(name, viewed))) {
        // what the note's tag holds below the one viewed moved with it
        const below = tagKey(old).slice(tagKey(viewed).length)
        for (const name of note.tags) {
          if (known.has(tagKey(name)) || !tagKey(name).endsWith(below)) continue
          const now = tagKey(name).slice(0, tagKey(name).length - below.length)
          const tag = this.tags.find((t) => tagKey(t.name) === now)
          if (tag) return tag
        }
      }
    }
    return null
  }

  private async follow() {
    const scope = this.scope
    const viewed = scope.kind === 'tag' ? this.tagName(scope.id) : ''
    const listed = this.notes
    const known = new Set(this.tags.map((tag) => tagKey(tag.name)))
    await this.load()
    if (scope.kind === 'tag' && viewed && !this.tags.some((tag) => tag.id === scope.id)) {
      const tag = await this.renamedTag(viewed, listed, known)
      if (tag && this.scope.kind === 'untagged') {
        this.scope = { kind: 'tag', id: tag.id }
        await this.refreshNotes()
      }
    }
    if (this.hits) await this.search(this.query)
    const note = this.current
    if (!note) return
    const follows = this.coediting()
    if (follows) await this.flush()
    let fresh: Note | null
    try {
      fresh = await backend.getNote(note.id)
    } catch {
      fresh = null
    }
    if (this.current?.id !== note.id) return
    if (!fresh || fresh.trashedAt) {
      // what was typed since goes into the note in the trash, and the note
      // stays open while that could not be saved
      if (fresh) await this.flush()
      if (this.current?.id !== note.id || (fresh && this.unsaved)) return
      clearTimeout(this.timer)
      this.pending = {}
      this.editedSince = null
      this.dirty = false
      this.unsaved = false
      this.show(null)
      if (fresh) toasts.push(fresh.title.trim() || t('note.untitled'), t('note.trashedElsewhere'))
      return
    }
    this.current.pinned = fresh.pinned
    this.current.tags = fresh.tags
    this.current.folderId = fresh.folderId
    if (!follows) return
    // keys typed while the note was read stay: the editor shows them, and
    // their save, made from the text they started from, brings the merge
    this.serializeEditor?.()
    if (this.dirty || this.saving) return
    if (fresh.title !== this.current.title || fresh.body !== this.current.body) this.adopt(fresh)
  }

  /**
   * Saves the open note before another takes its place. A note left with no
   * title and no text is not kept: creating one and backing out leaves
   * nothing behind. False, staying on the note, when its edit could not be
   * saved: the queued edit belongs to it.
   */
  private async leave(next: Id | null = null) {
    await this.flush()
    if (this.unsaved) return false
    const note = this.current
    if (!note || note.id === next || note.title.trim() || note.body.trim() || note.pinned)
      return true
    try {
      if (await backend.discardIfBlank(note.id)) await this.refreshNotes()
    } catch (err) {
      this.report(err)
    }
    return true
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
  async create(initial?: { title?: string; body?: string }, open?: true): Promise<Note | null>
  async create(initial: { title?: string; body?: string }, open: false): Promise<Note>
  async create(initial: { title?: string; body?: string } = {}, open = true) {
    if (open && !(await this.leave())) return null
    const tags = this.scope.kind === 'tag' ? [this.tagName(this.scope.id)] : []
    const note = await backend.createNote({ ...initial, tags })
    if (open) {
      this.fresh = note.id
      this.show(note)
    }
    await this.refreshNotes()
    return note
  }

  /**
   * Creates a note from each markdown file, in the current folder or tag,
   * and opens the last one. Other files are skipped and reported.
   */
  async importFiles(files: File[]) {
    if (!(await this.leave())) return
    const markdown = files.filter(isMarkdownFile)
    if (markdown.length < files.length) this.error = 'notMarkdown'
    if (markdown.length === 0) return
    const scopeTags = this.scope.kind === 'tag' ? [this.tagName(this.scope.id)] : []
    try {
      let last: Note | null = null
      for (const file of markdown) {
        const { title, body, tags, pinned } = parseMarkdownFile(file.name, await file.text())
        const allTags = [...new Set([...scopeTags, ...tags])]
        last = await backend.createNote({ title, body, tags: allTags, pinned })
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
    const now = Date.now()
    this.editedSince ??= now
    const left = this.editedSince + SAVE_AT_MOST_MS - now
    this.timer = setTimeout(() => void this.flush(), Math.max(0, Math.min(SAVE_DELAY_MS, left)))
  }

  /** The open editor's own save: it turns typing into markdown after a
   * short pause, so a flush asks it first (lock, close). */
  serializeEditor: (() => void) | null = null

  /** Saves a queued edit now. Called before leaving a note and on close.
   * A save still on its way ends first: the next one starts from what it
   * stored, so the same typing is never merged twice. */
  async flush() {
    this.serializeEditor?.()
    clearTimeout(this.timer)
    while (this.saving) await this.saving
    if (!this.current || Object.keys(this.pending).length === 0) return
    const save = this.save()
    this.saving = save
    try {
      await save
    } finally {
      this.saving = null
    }
  }

  private async save() {
    const note = this.current
    const change = this.pending
    if (!note) return
    this.pending = {}
    this.editedSince = null
    const base = this.base
    const sent: NoteChange = { ...change }
    if (base && change.title !== undefined) sent.baseTitle = base.title
    if (base && change.body !== undefined) sent.baseBody = base.body
    try {
      const saved = await this.store(note.id, sent)
      if (this.current?.id === saved.id) {
        // keys typed during the save are queued now, not after the editor's
        // own pause: text put in the editor below never replaces them
        this.serializeEditor?.()
        const merged =
          (change.title !== undefined && saved.title !== change.title) ||
          (change.body !== undefined && saved.body !== change.body)
        const queued = Object.keys(this.pending).length > 0
        this.unsaved = false
        if (!queued) {
          this.current.updatedAt = saved.updatedAt
          this.dirty = false
        }
        // another device's edit came in while this one was typed: with
        // co-editing the editor takes in the merge of both
        if (merged && !queued && this.coediting()) this.adopt(saved)
        // otherwise the editor goes on from the text it sent, which the
        // store holds merged now: the next save is that text's change,
        // never the same typing merged a second time. What was not sent
        // keeps the text it started from, whatever the store holds
        else if (base)
          this.base = {
            title: change.title ?? base.title,
            body: change.body ?? base.body,
          }
      }
      await Promise.all([this.refreshNotes(), this.refreshConflicts()])
      if (change.title !== undefined) await this.refreshTags()
    } catch (err) {
      // the edit is queued again, ahead of any typed since, and tried
      // again; a note deleted meanwhile has nothing to save to
      if (this.current?.id === note.id && !(isBackendError(err) && err.kind === 'notFound')) {
        this.pending = { ...change, ...this.pending }
        this.unsaved = true
        clearTimeout(this.timer)
        this.timer = setTimeout(() => void this.flush(), RETRY_DELAY_MS)
      }
      this.report(err)
    }
  }

  /** Stores an edit of the open note, in the trash too when another device
   * moved the note there while it was typed. */
  private async store(id: Id, sent: NoteChange) {
    try {
      return await backend.updateNote(id, sent)
    } catch (err) {
      if (!isBackendError(err) || err.kind !== 'invalid') throw err
      if (!(await backend.getNote(id)).trashedAt) throw err
      return backend.updateNote(id, { ...sent, inTrash: true })
    }
  }

  async setTags(tags: string[]) {
    if (!this.current) return
    await this.setNoteTags(this.current.id, tags)
  }

  async setNoteTags(id: Id, tags: string[]) {
    await backend.setNoteTags(id, tags)
    if (this.current?.id === id) this.current.tags = (await backend.getNote(id)).tags
    await this.refreshTagsAndNotes()
  }

  /** Gives a note one more tag, a note dropped on a tag for example. */
  async addTag(id: Id, name: string) {
    const note = await backend.getNote(id)
    if (note.tags.some((t) => tagKey(t) === tagKey(name))) return
    await this.setNoteTags(id, [...note.tags, name])
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

  /** Makes a tag that stays without notes, returning its id. */
  async createTag(name: string) {
    const id = await backend.createTag(name)
    await this.refreshTags()
    return id
  }

  /** Renames a tag or moves it with a path; the notes follow. */
  async renameTag(id: Id, name: string) {
    await backend.renameTag(id, name)
    await this.reloadTagged()
  }

  async setTagColor(id: Id, color: TagColor | null) {
    await backend.setTagColor(id, color)
    await this.refreshTags()
  }

  /** Takes a tag and the tags it holds off every note. */
  async deleteTag(id: Id) {
    await backend.deleteTag(id)
    await this.reloadTagged()
  }

  /** After a tag changed on many notes: the list, the open note's tags. */
  private async reloadTagged() {
    if (this.current) this.current.tags = (await backend.getNote(this.current.id)).tags
    await this.refreshTagsAndNotes()
  }

  tagName(id: Id): string {
    return this.tags.find((t) => t.id === id)?.name ?? ''
  }

  report(err: unknown) {
    this.error = isBackendError(err) ? err.kind : 'unknown'
    console.error(err)
  }
}

export const workspace = new Workspace()
