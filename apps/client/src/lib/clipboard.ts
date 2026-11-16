// SPDX-License-Identifier: AGPL-3.0-or-later
// Text on the clipboard: the system's through the app, the browser's own
// in a browser page (the web version), which may ask before it is read.

import {
  readText as readFromApp,
  writeText as writeFromApp,
} from '@tauri-apps/plugin-clipboard-manager'
import { backend } from './backend'

/** How long a button says Copied after copying, everywhere the same. */
export const COPIED_MS = 2000

export function copyText(text: string): Promise<void> {
  return backend.native ? writeFromApp(text) : navigator.clipboard.writeText(text)
}

export function pasteText(): Promise<string> {
  return backend.native ? readFromApp() : navigator.clipboard.readText()
}
