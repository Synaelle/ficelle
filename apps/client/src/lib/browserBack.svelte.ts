// SPDX-License-Identifier: AGPL-3.0-or-later
// The web version on a phone: back, the gesture or the browser's, closes
// what is open as it does in the Android app (back.ts), instead of leaving
// the page from a note, a sheet or a settings page. Each open layer has a
// history entry of its own, and back takes one off.

import { goBack, onStepsChanged, stepCount } from './back'
import { overlay } from './overlay.svelte'

/** History entries this page added above its own, one per open layer. */
let depth = 0
/** The page is taking entries off itself: that popstate is no back. */
let trimming = false
let timer = 0

const layers = () => stepCount() + (overlay.menu ? 1 : 0) + (overlay.dialog ? 1 : 0)

/** History follows the layers once they settle: a sheet closed for a dialog
 * in the same moment moves nothing. */
function follow() {
  clearTimeout(timer)
  timer = window.setTimeout(() => {
    if (trimming) return
    const open = layers()
    for (; depth < open; depth++) history.pushState({ layer: depth + 1 }, '')
    if (depth > open) {
      trimming = true
      history.go(open - depth)
      depth = open
    }
  })
}

/** Starts following the layers. Call once, on a phone in the web version. */
export function followBrowserBack() {
  onStepsChanged(follow)
  $effect.root(() => {
    $effect(() => {
      void overlay.menu
      void overlay.dialog
      follow()
    })
  })
  window.addEventListener('popstate', () => {
    if (trimming) {
      trimming = false
      return follow()
    }
    depth = Math.max(0, depth - 1)
    // nothing open: an entry left from before a reload, back goes on out
    if (!goBack()) history.back()
    follow()
  })
}
