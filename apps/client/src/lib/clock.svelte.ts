// SPDX-License-Identifier: AGPL-3.0-or-later
// 12 or 24 hours for every time the app shows. The settings may choose; by
// default the system says: Windows' and GNOME's clock (commands/clock.rs),
// Android's (MainActivity.kt), else the system's locale. The app's own
// language never decides it, an English app on a 24-hour system shows 14:30.
// No import of the preferences here, the admin page shares this file: the
// app copies its choice in (theme.svelte.ts).

export const clock = $state<{ choice: 'system' | '24' | '12'; system24h: boolean | null }>({
  choice: 'system',
  system24h: null,
})

/** The system's locale says 12 hours (en-US) or 24 (fr-FR, en-GB). */
function locale12h(): boolean {
  const cycle = new Intl.DateTimeFormat(navigator.language, { hour: 'numeric' }).resolvedOptions()
    .hourCycle
  return cycle === 'h11' || cycle === 'h12'
}

/** `hour12` for `Intl.DateTimeFormat` and `toLocaleString`. */
export function hour12(): boolean {
  if (clock.choice === '24') return false
  if (clock.choice === '12') return true
  return clock.system24h === null ? locale12h() : !clock.system24h
}

/** Reads the system's clock, `read` asking the desktop app. */
export async function readSystemClock(read: () => Promise<boolean | null>) {
  const android = window.AppSystem?.clock24h?.()
  try {
    clock.system24h = typeof android === 'boolean' ? android : await read()
  } catch {
    clock.system24h = null
  }
}
