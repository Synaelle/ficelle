// SPDX-License-Identifier: AGPL-3.0-or-later
// Size of text, buttons and spacing. The whole page is scaled like a browser
// zoom, so every size grows together and layouts reflow to the narrower
// width instead of each component carrying its own sizes. Phones scale
// through the viewport (Android's webview has no zoom), desktop through the
// webview zoom (desktop webviews ignore the viewport).

import { getCurrentWebview } from '@tauri-apps/api/webview'
import { backend } from './backend'

export type UiSize = 'small' | 'medium' | 'huge'
export const UI_SIZES: UiSize[] = ['small', 'medium', 'huge']

const phone = /android|iphone|ipad/i.test(navigator.userAgent)

/** Small is the original size. Huge keeps a 412 px phone at least 317 px wide. */
const SCALES: Record<UiSize, number> = phone
  ? { small: 1, medium: 1.075, huge: 1.3 }
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
      /** The system's accent as #RRGGBB, empty before Android 12. */
      accentColor?(): string
      /** The phone shows a 24-hour clock. */
      clock24h?(): boolean
      /** The folder this phone's backups go to, made if needed. */
      backupFolder?(): string
      setHidden?(on: boolean): void
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
  if (phone) {
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
  }
}
