// SPDX-License-Identifier: AGPL-3.0-or-later
// A short vibration when a gesture crosses its point of no return. Phones
// only, the plugin does nothing on desktop and the web build has no plugin.

import { impactFeedback, selectionFeedback } from '@tauri-apps/plugin-haptics'

/** A gesture will do something when released. */
export function tick() {
  void selectionFeedback().catch(() => undefined)
}

/** A gesture did something that changes data, like moving a note to the trash. */
export function thud() {
  void impactFeedback('medium').catch(() => undefined)
}
