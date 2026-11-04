// SPDX-License-Identifier: AGPL-3.0-or-later
// Locks an encrypted vault after a stretch without input. Time with the app
// in the background counts too, and a timer may stop while the computer
// sleeps, so the check compares clock times and never waits long: after a
// sleep the vault locks within a minute, sooner when the window shows again.
// Set to never, nothing runs.

import { prefs } from './prefs.svelte'

const CHECK_AT_MOST_EVERY_MS = 60_000
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

/** How long to wait before looking again: until the lock is due, at most
 * `CHECK_AT_MOST_EVERY_MS`. Null when it never locks. */
export function nextCheck(lastActive: number, now: number, minutes: number): number | null {
  const delay = lockDelay(minutes)
  if (delay === null) return null
  return Math.min(Math.max(lastActive + delay - now, 0), CHECK_AT_MOST_EVERY_MS)
}

/** Starts watching input. Call from a component, it stops with it. Choosing
 * another time starts over, which is activity anyway. */
export function autoLock(lock: () => void) {
  $effect(() => {
    const minutes = prefs.autoLockMinutes
    if (lockDelay(minutes) === null) return
    let lastActive = Date.now()
    let timer: ReturnType<typeof setTimeout> | undefined
    const touch = () => (lastActive = Date.now())
    const check = () => {
      clearTimeout(timer)
      timer = undefined
      const now = Date.now()
      // looked at again after locking too, in case the lock failed
      const idle = isIdle(lastActive, now, minutes)
      if (idle) lock()
      const wait = idle ? CHECK_AT_MOST_EVERY_MS : nextCheck(lastActive, now, minutes)
      if (wait !== null) timer = setTimeout(check, wait)
    }
    for (const name of ACTIVITY) window.addEventListener(name, touch, { passive: true })
    document.addEventListener('visibilitychange', check)
    window.addEventListener('focus', check)
    check()
    return () => {
      for (const name of ACTIVITY) window.removeEventListener(name, touch)
      document.removeEventListener('visibilitychange', check)
      window.removeEventListener('focus', check)
      clearTimeout(timer)
    }
  })
}
