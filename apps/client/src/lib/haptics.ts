// SPDX-License-Identifier: AGPL-3.0-or-later
// A short vibration when a gesture crosses its point of no return. Phones
// only, and only when turned on in Settings, Accessibility: the desktop apps
// have no haptics plugin, and their builds leave these calls out.

import { impactFeedback, selectionFeedback } from '@tauri-apps/plugin-haptics'
import { MOBILE } from './platform'
import { prefs } from './prefs.svelte'

/** A gesture will do something when released. */
export function tick() {
  if (MOBILE && prefs.haptics) void selectionFeedback().catch(() => undefined)
}

/** A gesture did something that changes data, like moving a note to the trash. */
export function thud() {
  if (MOBILE && prefs.haptics) void impactFeedback('medium').catch(() => undefined)
}
