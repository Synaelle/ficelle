// SPDX-License-Identifier: AGPL-3.0-or-later
// The system's accent color, for the setting that follows it: Android tells
// the page itself (SystemInfo in MainActivity.kt), the desktop apps ask the
// system (commands/accent.rs). Read when the setting is on, and again when
// the window comes back to the front, where the person may have changed it.

import { backend } from './backend'

export const systemAccent = $state<{ color: string | null; read: boolean }>({
  color: null,
  read: false,
})

let reading = false

/** Reads the system's accent now. `color` stays null where it has none. */
export async function readSystemAccent() {
  if (reading) return
  reading = true
  try {
    const android = window.AppSystem?.accentColor?.()
    const color = android !== undefined ? android : await backend.systemAccent()
    systemAccent.color = color && /^#[0-9A-F]{6}$/i.test(color) ? color.toUpperCase() : null
  } catch {
    systemAccent.color = null
  } finally {
    systemAccent.read = true
    reading = false
  }
}
