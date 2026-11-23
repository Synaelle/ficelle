// SPDX-License-Identifier: AGPL-3.0-or-later
// The web version loads parts of the page when they are first needed
// (Settings, an import's readers, a code block's language). One that failed
// to load, offline or after a new version replaced the files on the server,
// stays failed in this page: the browser and Vite keep the failure, a style
// sheet that failed is never asked again. So the page reloads, once its notes
// are saved and the server answers, or says why it cannot.

import { brand } from '@app/brand'
import { t } from './i18n.svelte'
import { toasts } from './toast.svelte'
import { workspace } from './workspace.svelte'

/** When the page last reloaded for a part, kept for the reloaded page. */
const RELOADED = 'partReloaded'
/** A part failing again this soon after a reload is not reloaded for: the
 * server itself fails, a loop would never end. */
const AGAIN_MS = 60_000

/** What Vite reported last, the error the import then throws. */
let failed: unknown = null
let checking: Promise<void> | null = null

/** Reloads the page when the server answers, else says the part needs a
 * connection. One check at a time. */
function reloadForPart(): Promise<void> {
  checking ??= (async () => {
    const reachable = await fetch('./', { method: 'HEAD', cache: 'no-store' }).then(
      (response) => response.ok,
      () => false,
    )
    if (!reachable)
      return toasts.push(
        brand.displayName,
        `${t('web.partOffline')} ${t('common.errorCode', { code: 'PART-OFFLINE' })}`,
      )
    const last = Number(sessionStorage.getItem(RELOADED) ?? 0)
    if (Date.now() - last < AGAIN_MS)
      return toasts.push(
        brand.displayName,
        `${t('web.partReload')} ${t('common.errorCode', { code: 'PART-RELOAD' })}`,
        { label: t('web.reload'), run: () => location.reload() },
      )
    sessionStorage.setItem(RELOADED, String(Date.now()))
    await workspace.flush()
    location.reload()
  })().finally(() => (checking = null))
  return checking
}

/** Starts watching the parts that fail to load. Call once from the root. */
export function followPageParts() {
  window.addEventListener('vite:preloadError', (event) => {
    failed = event.payload
    void reloadForPart()
  })
}

/** A part of the page, or null when it did not load: the page reloads or
 * says why, and the next ask tries again, never stuck on the failure. */
export async function loadPart<T>(load: () => Promise<T>): Promise<T | null> {
  // a failure before: this page cannot load it, whatever it asks
  if (failed) {
    await reloadForPart()
    return null
  }
  try {
    return await load()
  } catch (err) {
    if (err !== failed) throw err
    return null
  }
}
