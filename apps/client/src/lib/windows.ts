// SPDX-License-Identifier: AGPL-3.0-or-later
// Windows beside the main one (src-tauri/src/windows.rs): a file opened with
// the app (#file, FileWindow.svelte). It is the same app: unlocking the notes
// there opens them in the main window too.

import { invoke } from '@tauri-apps/api/core'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

const UNLOCKED = 'vault-unlocked'

export type WindowKind = { kind: 'main' } | { kind: 'file' }

export function windowKind(hash = location.hash): WindowKind {
  return hash === '#file' ? { kind: 'file' } : { kind: 'main' }
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

/** This window unlocked the notes: the others come back too. */
export const announceUnlocked = () => emit(UNLOCKED, { from: label() }).catch(() => {})

export function onUnlockedElsewhere(run: () => void): Promise<UnlistenFn> {
  return listen<{ from: string }>(UNLOCKED, (event) => {
    if (event.payload.from !== label()) run()
  })
}

/** From a file window: the main window comes forward on the imported note. */
export const showInMain = (id: string) => invoke('main_window_show', { id })

/** In the main window: the note a file became, to open once ready. */
export const takeNoteToOpen = () => invoke<string | null>('main_window_take_note')

export const onNoteToOpen = (run: () => void) => listen('note-to-open', run)
