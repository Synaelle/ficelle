// SPDX-License-Identifier: AGPL-3.0-or-later
// Picks the backend: tauri inside the app, in memory in a plain browser. Only
// dev builds (pnpm dev:web) fill it with the sample notes, so the ui can be
// worked on without the native shell; the server's admin page gets it empty.

import type { Backend } from './backend'
import { createMemoryBackend } from './memory'
import { createTauriBackend } from './tauri'

export type { Backend } from './backend'
export * from './types'

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

// dev only: ?fresh starts with no notes, on the first-run setup
const fresh =
  import.meta.env.DEV &&
  typeof location !== 'undefined' &&
  new URLSearchParams(location.search).has('fresh')

export const backend: Backend = inTauri
  ? createTauriBackend()
  : createMemoryBackend({ seed: import.meta.env.DEV && !fresh })
