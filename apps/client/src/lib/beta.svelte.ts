// SPDX-License-Identifier: AGPL-3.0-or-later
// What only the Beta channel has: the feedback button and a welcome shown
// once. Beta versions look like 0.4.0-beta.2. The browser dev server can
// pretend with ?beta (and ?mac for the Mac text), builds for people cannot.

import { MAC } from './platform'
import { prefs } from './prefs.svelte'

const pretend = import.meta.env.DEV ? new URLSearchParams(location.search) : null

/** Test tools of a dev or preview build, "Pretend this is a beta release":
 * the beta and update screens show and can be walked through, nothing is
 * ever checked, downloaded or installed. Read once at start, the switch
 * reloads the page. Never in a release build. */
export const pretendRelease = __APP_DEV_REPORTS__ && prefs.pretendBeta

export const isBeta =
  /-beta\.\d+$/.test(__APP_VERSION__) || !!pretend?.has('beta') || pretendRelease

/** The Mac app, which is always a beta: macOS in the desktop shell, not an iPad. */
export const isMacApp = MAC || !!pretend?.has('mac')

/** Phones keep the feedback button in the notes list, not floating over it. */
export const touchScreen = window.matchMedia('(pointer: coarse)').matches

class BetaFeedback {
  /** The feedback form is open in its dialog. */
  open = $state(false)
}

export const betaFeedback = new BetaFeedback()

/** Shows the welcome again at the next start in a beta, for when someone
 * switches to the Beta channel. */
export function resetBetaWelcome() {
  prefs.betaWelcomed = false
}
