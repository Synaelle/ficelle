// SPDX-License-Identifier: AGPL-3.0-or-later
// The name people know their device's verification by, and whether it may
// ask on its own.

import { t } from './i18n.svelte'

const onWindows = () => /windows/i.test(navigator.userAgent)

export function biometricMethod(): string {
  return onWindows() ? t('unlock.methodWindows') : t('unlock.methodDevice')
}

/** Phones ask for the finger as the lock screen shows, the way they unlock
 * themselves. Windows Hello opens a window over everything, so it waits
 * for the button, also right after locking. */
export function asksOnItsOwn(): boolean {
  return !onWindows()
}
