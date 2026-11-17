// SPDX-License-Identifier: AGPL-3.0-or-later
// Windows beside the main one (src-tauri/src/windows.rs): a file opened with
// the app (#file, FileWindow.svelte) and a note in a window of its own
// (#note=<id>, NoteWindow.svelte, desktops only). They are one app: a change
// made in one window shows in the others, and every window locks together.

import { invoke } from '@tauri-apps/api/core'
import { emit, emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { DESKTOP } from './platform'

/** Notes, folders or tags were written in another window. */
const CHANGED = 'notes-changed'
/** Sent by the native side when the notes lock, from any window. */
const LOCKED = 'vault-locked'
const UNLOCKED = 'vault-unlocked'
/** Before a lock: each note window saves what it has queued. */
const FLUSH = 'notes-flush'
const FLUSHED = 'notes-flushed'
/** Typing in a note window keeps the main window's auto-lock away. */
const ACTIVITY = 'note-window-activity'
const ACTIVITY_EVENT = 'app-activity'

const FLUSH_WAIT_MS = 2000
const ACTIVITY_EVERY_MS = 15_000

/** Desktop apps open notes in their own window, phones and the web do not. */
export const noteWindowsSupported = DESKTOP

export type WindowKind = { kind: 'main' } | { kind: 'file' } | { kind: 'note'; id: string }

export function windowKind(hash = location.hash): WindowKind {
  if (hash === '#file') return { kind: 'file' }
  const note = /^#note=([0-9A-Za-z-]+)$/.exec(hash)?.[1]
  return note ? { kind: 'note', id: note } : { kind: 'main' }
}

const label = () => getCurrentWindow().label

export interface OpenedFile {
  name: string
  text: string
  /** A .txt file, shown and saved as plain text. */
  plain: boolean
  /** When the file was last changed on disk, null where the system does not say. */
  stamp: string | null
}

/** The file this window was opened for. */
export const readOpenedFile = () => invoke<OpenedFile>('opened_file_read')

/** Writes this window's file, gives its new stamp. Fails with
 * `changedOnDisk` when another app changed it since `stamp`; a null stamp
 * writes over it. */
export const writeOpenedFile = (text: string, stamp: string | null) =>
  invoke<string | null>('opened_file_write', { text, stamp })

/** Android: a file another app handed over becomes this window's file. */
export const takeOpenedFile = (address: string) => invoke('opened_file_take', { address })

/** The page is drawn, its window shows. */
export const windowReady = () => invoke('window_ready')

/** The note this window shows now, so no other window opens it too. */
export function showingNote(id: string | null) {
  if (!noteWindowsSupported) return
  void invoke('window_note_set', { id }).catch(() => {})
}

/** Brings forward another window that shows this note. False when none
 * does and this window may open it. */
export async function focusNoteWindow(id: string): Promise<boolean> {
  if (!noteWindowsSupported) return false
  return invoke<boolean>('note_window_focus', { id })
}

export const openNoteWindow = (id: string, title: string) =>
  invoke('note_window_open', { id, title })

/** Commands that write notes, folders, tags or files. */
const WRITES = new Set([
  'create_note',
  'update_note',
  'set_pinned',
  'move_note',
  'set_note_tags',
  'trash_note',
  'restore_note',
  'purge_note',
  'discard_if_blank',
  'empty_trash',
  'create_folder',
  'rename_folder',
  'move_folder',
  'trash_folder',
  'restore_folder',
  'create_tag',
  'rename_tag',
  'set_tag_color',
  'delete_tag',
  'restore_version',
  'resolve_conflict',
  'add_attachment',
  'remove_attachment',
  'attach_inbox_file',
  'import_write',
])

/** `invoke` that tells the other windows after a write. */
export const announcingInvoke: typeof invoke = async (command, args, options) => {
  const done = await invoke(command, args, options)
  if (noteWindowsSupported && WRITES.has(command))
    void emit(CHANGED, { from: label() }).catch(() => {})
  return done as never
}

/** Runs after another window wrote. */
export function onChangedElsewhere(run: () => void): Promise<UnlistenFn> {
  return listen<{ from: string }>(CHANGED, (event) => {
    if (event.payload.from !== label()) run()
  })
}

/** Runs when the notes lock, whichever window locked them. */
export const onLocked = (run: () => void) => listen(LOCKED, run)

/** This window unlocked the notes: the others come back too. */
export const announceUnlocked = () => emit(UNLOCKED, { from: label() }).catch(() => {})

export function onUnlockedElsewhere(run: () => void): Promise<UnlistenFn> {
  return listen<{ from: string }>(UNLOCKED, (event) => {
    if (event.payload.from !== label()) run()
  })
}

/** Before locking: every note window saves its queued edit first. Waits for
 * them, a window that does not answer in time locks without it. */
export async function flushNoteWindows() {
  if (!noteWindowsSupported) return
  const count = await invoke<number>('note_window_count').catch(() => 0)
  if (count === 0) return
  let answered = 0
  let stop: UnlistenFn | undefined
  await new Promise<void>((resolve) => {
    setTimeout(resolve, FLUSH_WAIT_MS)
    void listen(FLUSHED, () => {
      answered += 1
      if (answered >= count) resolve()
    }).then((unlisten) => {
      stop = unlisten
      void emit(FLUSH)
    })
  })
  stop?.()
}

/** In a note window: saves when a lock asks for it. */
export function onFlushAsked(flush: () => Promise<void>): Promise<UnlistenFn> {
  return listen(FLUSH, async () => {
    await flush().catch(() => {})
    await emit(FLUSHED)
  })
}

/** In a note window: tells the main window's auto-lock the person is here. */
export function reportActivity() {
  let last = 0
  const touch = () => {
    const now = Date.now()
    if (now - last < ACTIVITY_EVERY_MS) return
    last = now
    void emitTo('main', ACTIVITY).catch(() => {})
  }
  for (const name of ['pointerdown', 'keydown', 'wheel'])
    window.addEventListener(name, touch, { passive: true })
}

/** In the main window: activity in a note window counts as its own. */
export function followActivity(): Promise<UnlistenFn> {
  return listen(ACTIVITY, () => window.dispatchEvent(new Event(ACTIVITY_EVENT)))
}

/** From a file window: the main window comes forward on the imported note. */
export const showInMain = (id: string) => invoke('main_window_show', { id })

/** In the main window: the note a file became, to open once ready. */
export const takeNoteToOpen = () => invoke<string | null>('main_window_take_note')

export const onNoteToOpen = (run: () => void) => listen('note-to-open', run)
