// SPDX-License-Identifier: AGPL-3.0-or-later
// The file viewer's calls to the native side (src-tauri/src/opened_files.rs,
// whose comment lists everything to take out with it).

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export interface OpenedFile {
  name: string
  text: string
  /** A .txt file, shown and saved as plain text. */
  plain: boolean
  /** When the file was last changed on disk, null where the system does not say. */
  stamp: string | null
  /** Made read-only by its owner: never written. */
  readOnly: boolean
}

/** The file this window was opened for. */
export const readOpenedFile = () => invoke<OpenedFile>('opened_file_read')

/** Writes this window's file, gives its new stamp. Fails with
 * `changedOnDisk` when another app changed it since `stamp`; a null stamp
 * writes over it. */
export const writeOpenedFile = (text: string, stamp: string | null) =>
  invoke<string | null>('opened_file_write', { text, stamp })

/** Whether the main window is open, whose auto-lock covers the notes. */
export const mainWindowOpen = () => invoke<boolean>('main_window_open')

/** Android: a file another app handed over becomes this window's file. */
export const takeOpenedFile = (address: string) => invoke('opened_file_take', { address })

/** From a file window: the main window comes forward on the imported note. */
export const showInMain = (id: string) => invoke('main_window_show', { id })

/** In the main window: the note a file became, to open once ready. */
export const takeNoteToOpen = () => invoke<string | null>('main_window_take_note')

export const onNoteToOpen = (run: () => void) => listen('note-to-open', run)

declare global {
  interface Window {
    /** Called by MainActivity when a file is opened with the app. */
    __appFile?: () => void
  }
}

/** Android: the addresses of the files opened with the app ("open with"),
 * waiting apart from the rest: they show even with the notes locked. */
export function takeOpenedFiles(): string[] {
  try {
    const addresses: unknown = JSON.parse(window.AppInbox?.takeFiles() ?? '[]')
    return Array.isArray(addresses) ? addresses.filter((a) => typeof a === 'string') : []
  } catch {
    return []
  }
}
