// SPDX-License-Identifier: AGPL-3.0-or-later
// Picks the backend: tauri inside the app, the notes core in a worker in the
// web version, the core in the page with its notes in memory in any other
// browser page (pnpm dev:web, the tests). Dev builds fill that one with the
// sample notes, so the ui can be worked on without the native shell.

import { PLATFORM, WEB } from '../platform'
import { announcingInvoke } from '../windows'
import type { Backend } from './backend'
import { createPageBackend } from './pageCore'
import { createTauriBackend } from './tauri'
import { createWebBackend } from './web'

export type { Backend } from './backend'
export * from './types'

// an app build is always inside tauri: the other backends are left out
const inTauri =
  PLATFORM !== 'web' || (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window)

// dev only: ?fresh starts with no notes, on the first-run setup
const fresh =
  import.meta.env.DEV &&
  typeof location !== 'undefined' &&
  new URLSearchParams(location.search).has('fresh')

export const backend: Backend = inTauri
  ? createTauriBackend(announcingInvoke)
  : WEB
    ? createWebBackend()
    : createPageBackend({ seed: import.meta.env.DEV && !fresh })
