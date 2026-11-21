// SPDX-License-Identifier: AGPL-3.0-or-later
// What only early versions have: the feedback button and a welcome shown
// once. A nightly looks like nightly-2026-11-16, an older beta like
// 0.4.0-beta.2. The browser dev server can pretend with ?beta, builds for
// people cannot.

import { prefs } from './prefs.svelte'

const pretend = import.meta.env.DEV ? new URLSearchParams(location.search) : null

/** Test tools of a dev or preview build, "Pretend this is a beta release":
 * the beta and update screens show and can be walked through, nothing is
 * ever checked, downloaded or installed. Read once at start, the switch
 * reloads the page. Never in a release build. */
export const pretendRelease = __APP_DEV_TOOLS__ && prefs.pretendBeta

/** A nightly's name, `.2` and on for a fix the same day. */
export const NIGHTLY = /^nightly-\d{4}-\d{2}-\d{2}(\.\d+)?$/

export const isBeta =
  /-beta\.\d+$/.test(__APP_VERSION__) ||
  NIGHTLY.test(__APP_VERSION__) ||
  !!pretend?.has('beta') ||
  pretendRelease

/** Phones keep the feedback button in the notes list, not floating over it. */
export const touchScreen = window.matchMedia('(pointer: coarse)').matches

/** Shows the welcome again at the next start in a nightly, for when someone
 * switches to the Nightly channel. */
export function resetBetaWelcome() {
  prefs.betaWelcomed = false
}
