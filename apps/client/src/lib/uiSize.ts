// SPDX-License-Identifier: AGPL-3.0-or-later
// Size of text, buttons and spacing. The whole page is scaled like a browser
// zoom, so every size grows together and layouts reflow to the narrower
// width instead of each component carrying its own sizes. The Android app
// scales through the viewport (its webview has no zoom), desktop through the
// webview zoom (desktop webviews ignore the viewport), the web version through
// CSS zoom: a phone's browser keeps the layout as wide as the screen when the
// viewport scales, and zooms in past its edges.

import { getCurrentWebview } from '@tauri-apps/api/webview'
import { backend } from './backend'

export type UiSize = 'small' | 'medium' | 'huge'
export const UI_SIZES: UiSize[] = ['small', 'medium', 'huge']

const phone = /android|iphone|ipad/i.test(navigator.userAgent)

/** Small is the original size. Huge keeps a 412 px phone at least 317 px wide. */
const SCALES: Record<UiSize, number> = phone
  ? { small: 1, medium: 1.02, huge: 1.3 }
  : { small: 1, medium: 1.1, huge: 1.25 }
/** Layouts are checked down to 317 px, the width of Huge on a usual phone. */
const MAX_PHONE_SCALE = 1.3

declare global {
  interface Window {
    /** Set by MainActivity on Android. */
    AppSystem?: {
      fontScale(): number
      /** Notes out of screenshots and the recent apps (FLAG_SECURE). */
      hidden?(): boolean
      /** The phone shows a 24-hour clock. */
      clock24h?(): boolean
      /** The folder this phone's backups go to, made if needed. */
      backupFolder?(): string
      setHidden?(on: boolean): void
      /** The colour behind the status and navigation bars, and their icons. */
      setBars?(color: string, light: boolean): void
    }
  }
}

/** Android's font size setting, 1 elsewhere. */
function systemFontScale(): number {
  const scale = window.AppSystem?.fontScale()
  return typeof scale === 'number' && scale > 0 ? scale : 1
}

export function applyUiSize(size: UiSize) {
  const chosen = SCALES[size] ?? SCALES.medium
  // the system font size multiplies the chosen size, as in other apps
  const scale = phone ? Math.min(chosen * systemFontScale(), MAX_PHONE_SCALE) : chosen
  if (phone && backend.native) {
    // the minimum keeps the layout width, pinching only magnifies
    document
      .querySelector('meta[name=viewport]')
      ?.setAttribute(
        'content',
        `width=device-width, initial-scale=${scale}, minimum-scale=${scale}, maximum-scale=5, viewport-fit=cover, interactive-widget=resizes-content`,
      )
  } else if (backend.native) {
    void getCurrentWebview()
      .setZoom(scale)
      .catch(() => {})
  } else {
    // a browser page has no webview zoom: the page zooms itself
    document.documentElement.style.zoom = String(scale)
    if (followingHeight) showVisibleHeight()
  }
}

/** The web version on a phone follows the height in sight, see
 * followVisibleHeight. */
let followingHeight = false

/** The page as tall as what is in sight: a keyboard that covers the page
 * instead of shrinking it (Safari) leaves the page the height still in sight.
 * Times the pinch zoom, so pinching in does not shrink the page with it, and
 * over the page's own zoom, which applies to the height again. */
function showVisibleHeight() {
  const viewport = window.visualViewport
  if (!viewport) return
  document.documentElement.style.setProperty(
    '--visible-height',
    `${(viewport.height * viewport.scale) / pageZoom()}px`,
  )
}

/** For the web version on a phone: the page keeps to the height in sight. */
export function followVisibleHeight() {
  const viewport = window.visualViewport
  if (!viewport) return
  followingHeight = true
  viewport.addEventListener('resize', showVisibleHeight)
  showVisibleHeight()
}

/** The page's own zoom (the web version on a computer, see applyUiSize), 1
 * elsewhere. What the page measures (boxes, pointer positions) is zoomed,
 * what css places is not: a measured position divided by this is placed
 * where it was measured. */
export function pageZoom(): number {
  return Number(document.documentElement.style.zoom) || 1
}
