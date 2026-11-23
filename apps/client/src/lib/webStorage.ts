// SPDX-License-Identifier: AGPL-3.0-or-later
// Imported first by main.ts: in the web version, the page's localStorage and
// sessionStorage become its scope's own (webScope.ts) before any other module
// reads them. The apps keep theirs as they are.

import { pageScope, scopedStorage } from './webScope'

/** The browser lets this site keep nothing (Firefox with site data blocked
 * throws on the first touch of localStorage): the page starts on storages
 * kept in memory, and App.svelte says the notes cannot be kept. */
export let storageRefused = false

/** A storage that lasts as long as the page. */
function memoryStorage(): Storage {
  const items = new Map<string, string>()
  return {
    get length() {
      return items.size
    },
    key: (index) => [...items.keys()][index] ?? null,
    getItem: (key) => items.get(key) ?? null,
    setItem: (key, value) => void items.set(key, String(value)),
    removeItem: (key) => void items.delete(key),
    clear: () => items.clear(),
  }
}

if (__APP_WEB__) {
  const scope = pageScope()
  for (const name of ['localStorage', 'sessionStorage'] as const) {
    let real: Storage
    try {
      real = window[name]
      void real.length
    } catch {
      storageRefused = true
      real = memoryStorage()
    }
    const scoped = scopedStorage(real, scope)
    Object.defineProperty(window, name, { configurable: true, get: () => scoped })
  }
}
