// SPDX-License-Identifier: AGPL-3.0-or-later
// Runs on the in-memory backend, the vault standing in for every device.
import { flushSync } from 'svelte'
import { describe, expect, it } from 'vitest'
import { backend } from './backend'
import { prefs } from './prefs.svelte'
import { sendSyncedPrefs, takeSyncedPrefs } from './syncedPrefs.svelte'

const stored = async () => Object.fromEntries(await backend.syncedSettings())

/** A tick of the backend's promises. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

describe('choices that follow the person', () => {
  it('sends only what changes here, never what was there before', async () => {
    prefs.sort = 'createdDesc'
    const stop = $effect.root(() => sendSyncedPrefs())
    flushSync()
    await settle()
    expect(await stored()).toEqual({})
    prefs.autoLists = false
    prefs.uiSize = 'huge'
    flushSync()
    await settle()
    expect(await stored()).toEqual({ autoLists: 'false' })
    stop()
  })

  it("takes another device's choice without sending it back", async () => {
    const stop = $effect.root(() => sendSyncedPrefs())
    flushSync()
    await backend.setSyncedSetting('mode', '"light"')
    await takeSyncedPrefs()
    flushSync()
    await settle()
    expect(prefs.mode).toBe('light')
    expect((await stored()).mode).toBe('"light"')
    stop()
  })

  it('leaves out what this version does not know and what stays per device', async () => {
    prefs.sort = 'updatedDesc'
    prefs.language = null
    await backend.setSyncedSetting('sort', '"byColour"')
    await backend.setSyncedSetting('clock', '24')
    await backend.setSyncedSetting('language', '"fr"')
    await takeSyncedPrefs()
    expect(prefs.sort).toBe('updatedDesc')
    expect(prefs.clock).not.toBe(24)
    expect(prefs.language).toBeNull()
  })
})
