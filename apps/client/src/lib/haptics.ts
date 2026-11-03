// SPDX-License-Identifier: AGPL-3.0-or-later
// A short vibration when a gesture crosses its point of no return. Phones
// only: the desktop apps have no haptics plugin, and their builds leave
// these calls out.

import { impactFeedback, selectionFeedback } from '@tauri-apps/plugin-haptics'
import { MOBILE } from './platform'

/** A gesture will do something when released. */
export function tick() {
  if (MOBILE) void selectionFeedback().catch(() => undefined)
}

/** A gesture did something that changes data, like moving a note to the trash. */
export function thud() {
  if (MOBILE) void impactFeedback('medium').catch(() => undefined)
}
