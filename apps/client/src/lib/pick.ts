// SPDX-License-Identifier: AGPL-3.0-or-later
// The system's file pickers. In dev builds the end to end tests answer
// through `window.__testPick`: a real picker would open on the screen of
// whoever runs them.

import {
  open,
  save,
  type OpenDialogOptions,
  type SaveDialogOptions,
} from '@tauri-apps/plugin-dialog'

const testPick = () =>
  __APP_DEV_REPORTS__ ? (window as { __testPick?: string }).__testPick : undefined

// Android turns extensions into media types, and greys out every file whose
// type it does not know (`.enex`, a backup): everything is offered there,
// the app tells formats apart itself
const android = /android/i.test(navigator.userAgent)

/** A file or folder to read, null when the person cancels. */
export async function pickToOpen(options: OpenDialogOptions): Promise<string | null> {
  const offered = android ? { ...options, filters: undefined } : options
  const picked = testPick() ?? (await open({ ...offered, multiple: false }))
  return typeof picked === 'string' ? picked : null
}

/** Where to write a file, null when the person cancels. */
export async function pickToSave(options: SaveDialogOptions): Promise<string | null> {
  return testPick() ?? (await save(options))
}
