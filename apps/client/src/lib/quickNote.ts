// SPDX-License-Identifier: AGPL-3.0-or-later
// Desktop quick note: a shortcut that works in any app opens a small window
// to jot a note without going through the main one. Off by default, the
// binding lives in prefs. The window is the same page with #quick, see
// QuickNote.svelte.

import { emitTo, listen } from '@tauri-apps/api/event'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { register, unregister } from '@tauri-apps/plugin-global-shortcut'
import { backend } from './backend'
import { t } from './i18n.svelte'

export const QUICK_LABEL = 'quick'
/** Suggested when the shortcut is turned on. */
export const DEFAULT_QUICK_SHORTCUT = 'Mod+Alt+N'
/** Sent to the main window after a quick note is saved. */
const SAVED_EVENT = 'quick-note-saved'

export const quickNoteSupported =
  backend.native && !/android|iphone|ipad/i.test(navigator.userAgent)

/** The binding of a key press for a global shortcut, from the physical key. */
export function globalBindingOf(event: KeyboardEvent): string | null {
  const key = event.code.match(/^(?:Key|Digit)(.)$/)?.[1] ?? event.code.match(/^F\d{1,2}$/)?.[0]
  if (!key) return null
  const parts: string[] = []
  if (event.ctrlKey || event.metaKey) parts.push('Mod')
  if (event.altKey) parts.push('Alt')
  if (event.shiftKey) parts.push('Shift')
  // a bare letter would fire while typing in every other app
  if (parts.length === 0 || (parts.length === 1 && parts[0] === 'Shift')) return null
  return [...parts, key].join('+')
}

const toAccelerator = (binding: string) => binding.replace('Mod', 'CommandOrControl')

let registered: string | null = null

/**
 * Makes the shortcut match the preference. Returns false when the system
 * refused it, usually because another app holds the same keys.
 */
export async function applyQuickShortcut(binding: string | null): Promise<boolean> {
  if (!quickNoteSupported || binding === registered) return true
  if (registered) await unregister(toAccelerator(registered)).catch(() => {})
  registered = null
  if (!binding) return true
  try {
    await register(toAccelerator(binding), (event) => {
      if (event.state === 'Pressed') void openQuickNote()
    })
    registered = binding
    return true
  } catch {
    return false
  }
}

export async function openQuickNote() {
  const existing = await WebviewWindow.getByLabel(QUICK_LABEL)
  if (existing) {
    await existing.unminimize()
    await existing.show()
    await existing.setFocus()
    return
  }
  await backend.quickWindowOpen(t('quick.title'))
}

export const isQuickWindow = () => location.hash === '#quick'

/** From the quick window: the main window should show the new note. */
export function announceSaved() {
  return emitTo('main', SAVED_EVENT)
}

/** In the main window: runs after each quick note, returns the unlisten. */
export function onQuickNoteSaved(run: () => void) {
  return listen(SAVED_EVENT, run)
}
