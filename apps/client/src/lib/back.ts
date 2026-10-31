// SPDX-License-Identifier: AGPL-3.0-or-later
// Android's back button and gesture. The page uses no browser history, so
// MainActivity asks here first: each open layer (a note on a phone, settings,
// a sub page, a sheet) adds a step, and the newest runs. With nothing open
// Android leaves the app as usual.

import { overlay } from './overlay.svelte'

const steps: (() => void)[] = []

/** Adds a back step while a layer is open, returns its removal for $effect. */
export function onBack(step: () => void): () => void {
  steps.push(step)
  return () => {
    const at = steps.lastIndexOf(step)
    if (at >= 0) steps.splice(at, 1)
  }
}

/** Runs the newest step, false when there is nothing left to close. */
export function goBack(): boolean {
  // menus and dialogs sit above every layer and register nothing
  if (overlay.menu) overlay.closeMenu()
  else if (overlay.dialog) overlay.answer(null)
  else {
    const step = steps.at(-1)
    if (!step) return false
    step()
  }
  return true
}

declare global {
  interface Window {
    __appBack?: () => boolean
  }
}
window.__appBack = goBack
