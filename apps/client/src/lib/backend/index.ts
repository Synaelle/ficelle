// SPDX-License-Identifier: AGPL-3.0-or-later
// Picks the backend: tauri inside the app, in-memory sample data in a plain
// browser (pnpm dev:web) so the ui can be worked on without the native shell.

import type { Backend } from './backend'
import { createMemoryBackend } from './memory'
import { createTauriBackend } from './tauri'

export type { Backend } from './backend'
export * from './types'

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const backend: Backend = inTauri ? createTauriBackend() : createMemoryBackend({ seed: true })
