// SPDX-License-Identifier: AGPL-3.0-or-later
// The system's file pickers. In dev builds the end to end tests answer
// through `window.__testPick`: a real picker would open on the screen of
// whoever runs them. The web version has no paths: a picked file is kept
// here under a made up one the backend reads it by (`pickedFile`), and a
// file to save is named only, the backend hands it to the browser as a
// download.

import {
  open,
  save,
  type OpenDialogOptions,
  type SaveDialogOptions,
} from '@tauri-apps/plugin-dialog'
import { WEB } from './platform'

const testPick = () =>
  __APP_DEV_TOOLS__ ? (window as { __testPick?: string }).__testPick : undefined

// Android turns extensions into media types, and greys out every file whose
// type it does not know (`.enex`, a backup): everything is offered there,
// the app tells formats apart itself
const android = /android/i.test(navigator.userAgent)

/** Files picked in the web version, by the path made up for them. */
const picked = new Map<string, File>()

/** A file the web version's picker gave, by its made up path. */
export const pickedFile = (path: string): File | undefined => picked.get(path)

/** The browser's own file picker, one file. */
function pickInBrowser(options: OpenDialogOptions): Promise<string | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input')
    input.type = 'file'
    const types = options.filters?.flatMap((f) => f.extensions) ?? []
    if (types.length > 0 && !types.includes('*')) input.accept = types.map((e) => `.${e}`).join(',')
    input.onchange = () => {
      const file = input.files?.[0]
      if (!file) return resolve(null)
      const path = `picked/${picked.size}/${file.name}`
      picked.set(path, file)
      resolve(path)
    }
    input.oncancel = () => resolve(null)
    input.click()
  })
}

/** A file or folder to read, null when the person cancels. */
export async function pickToOpen(options: OpenDialogOptions): Promise<string | null> {
  if (WEB) return pickInBrowser(options)
  const offered = android ? { ...options, filters: undefined } : options
  const chosen = testPick() ?? (await open({ ...offered, multiple: false }))
  return typeof chosen === 'string' ? chosen : null
}

/** Where to write a file, null when the person cancels. */
export async function pickToSave(options: SaveDialogOptions): Promise<string | null> {
  // the browser asks where, or puts it in Downloads, once the file is made
  if (WEB) return options.defaultPath ?? 'download'
  return testPick() ?? (await save(options))
}
