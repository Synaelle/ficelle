// SPDX-License-Identifier: AGPL-3.0-or-later
// The web version's own corner of the browser. A browser keeps one storage per
// address, not per path: two web versions on one address (a public one at
// /web, a server's at /app) would otherwise open the same notes. Everything the
// web version keeps is named after the path it is served from: its notes (the
// core's folder, see core.worker.ts), its settings (scopedStorage), the lock
// between its tabs. Two paths never share any of it.

/** The scope of a page served at `path`: the path itself, encoded, so two
 * paths never give the same scope and the root has one too. Null for
 * anything that would not be a plain name. */
export function scopeOf(path: string): string | null {
  const trimmed = path.replace(/^\/+|\/+$/g, '')
  const scope = encodeURIComponent(`/${trimmed}`)
  return /^[A-Za-z0-9%._~-]{1,200}$/.test(scope) ? scope : null
}

/** This page's scope, from where it is served. */
export function pageScope(): string {
  const scope = scopeOf(new URL('.', document.baseURI).pathname)
  // never a shared fallback: without its own scope the web version opens nothing
  if (!scope) throw new Error('the web version cannot tell where it is served from')
  return scope
}

/** `real` seen through `scope`: every key read, written, listed or cleared is
 * this scope's own, another scope's keys are out of reach, even for clear(). */
export function scopedStorage(real: Storage, scope: string): Storage {
  const prefix = `ficelle:${scope}:`
  const own = () =>
    Array.from({ length: real.length }, (_, i) => real.key(i)).filter(
      (key): key is string => key?.startsWith(prefix) ?? false,
    )
  return {
    get length() {
      return own().length
    },
    key: (index) => own()[index]?.slice(prefix.length) ?? null,
    getItem: (key) => real.getItem(prefix + key),
    setItem: (key, value) => real.setItem(prefix + key, value),
    removeItem: (key) => real.removeItem(prefix + key),
    clear: () => {
      for (const key of own()) real.removeItem(key)
    },
  }
}
