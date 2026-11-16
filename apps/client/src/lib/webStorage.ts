// SPDX-License-Identifier: AGPL-3.0-or-later
// Imported first by main.ts: in the web version, the page's localStorage and
// sessionStorage become its scope's own (webScope.ts) before any other module
// reads them. The apps keep theirs as they are.

import { pageScope, scopedStorage } from './webScope'

if (__APP_WEB__) {
  const scope = pageScope()
  for (const name of ['localStorage', 'sessionStorage'] as const) {
    const scoped = scopedStorage(window[name], scope)
    Object.defineProperty(window, name, { configurable: true, get: () => scoped })
  }
}
