// SPDX-License-Identifier: AGPL-3.0-or-later
// A picture of the app as it shows now, for an error report (sent only when
// ticked) and the dev reports: the webview's own picture on phones
// (Screenshot.kt), drawn from the page on computers.

declare global {
  interface Window {
    AppShot?: { take(): string }
  }
}

/** Elements with this attribute stay out of the picture (a report button). */
export const NO_SHOT = 'data-no-shot'

/** Null when it failed, the report goes without. */
export async function screenshot(): Promise<File | null> {
  try {
    const android = window.AppShot
    if (android) {
      const base64 = android.take()
      if (!base64) return null
      const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0))
      return new File([bytes], 'screenshot.png', { type: 'image/png' })
    }
    // loaded only when a picture is wanted
    const { domToBlob } = await import('modern-screenshot')
    const blob = await domToBlob(document.documentElement, {
      scale: window.devicePixelRatio,
      width: window.innerWidth,
      height: window.innerHeight,
      filter: (node) => !(node instanceof Element && node.hasAttribute(NO_SHOT)),
    })
    return new File([blob], 'screenshot.png', { type: 'image/png' })
  } catch (err) {
    console.warn('screenshot failed', err)
    return null
  }
}
