// SPDX-License-Identifier: AGPL-3.0-or-later
// The notes core (crates/core-web) run in the page itself, its notes in
// memory and gone with the page: the browser preview of development (pnpm
// dev:web) and the tests, on the same store as the apps. Only the web
// version keeps notes in the browser, from its worker (web.ts).

import init, { call, callBytes, readBytes, startInMemory } from '@app/core-web'
import type { Backend } from './backend'
import { coreError } from './coreError'
import { sampleNotes } from './sample'
import { coreBackend, type Send } from './web'

/** Runs a command on the core, loaded already. */
const run: Send = async <T>(
  name: string,
  args: Record<string, unknown> = {},
  more: { bytes?: Uint8Array; binary?: boolean } = {},
) => {
  const json = JSON.stringify(args)
  try {
    if (more.binary) return readBytes(name, json) as T
    const text = more.bytes ? callBytes(name, json, more.bytes) : call(name, json)
    return JSON.parse(text) as T
  } catch (err) {
    throw coreError(err)
  }
}

/** The core, loaded once for the page, `seed` with the sample notes in an
 * unencrypted vault. */
export function createPageBackend(options: { seed?: boolean } = {}): Backend {
  let loaded: Promise<void> | null = null
  const load = () =>
    (loaded ??= init().then(async () => {
      startInMemory()
      if (!options.seed) return
      await run('vault_create', { way: 'plain', password: '' })
      for (const note of sampleNotes()) await run('create_note', { note })
    }))
  const send: Send = async (name, args, more) => {
    await load()
    return run(name, args, more)
  }
  return coreBackend(send)
}
