// SPDX-License-Identifier: AGPL-3.0-or-later
// Locks an encrypted vault after a stretch without input. Time with the app
// in the background counts too, timers are throttled there, so the check
// compares clock times instead of trusting a single long timeout.

import { prefs } from './prefs.svelte'

const CHECK_EVERY_MS = 15_000
const ACTIVITY = ['pointerdown', 'keydown', 'wheel', 'touchstart'] as const

/** Idle time in ms after which to lock, or null for never. */
export function lockDelay(minutes: number): number | null {
  return minutes > 0 ? minutes * 60_000 : null
}

/** True when the vault should lock now. */
export function isIdle(lastActive: number, now: number, minutes: number): boolean {
  const delay = lockDelay(minutes)
  return delay !== null && now - lastActive >= delay
}

/** Starts watching input. Call from a component, it stops with it. */
export function autoLock(lock: () => void) {
  $effect(() => {
    let lastActive = Date.now()
    const touch = () => (lastActive = Date.now())
    const check = () => {
      if (isIdle(lastActive, Date.now(), prefs.autoLockMinutes)) lock()
    }
    for (const name of ACTIVITY) window.addEventListener(name, touch, { passive: true })
    document.addEventListener('visibilitychange', check)
    const timer = setInterval(check, CHECK_EVERY_MS)
    return () => {
      for (const name of ACTIVITY) window.removeEventListener(name, touch)
      document.removeEventListener('visibilitychange', check)
      clearInterval(timer)
    }
  })
}
