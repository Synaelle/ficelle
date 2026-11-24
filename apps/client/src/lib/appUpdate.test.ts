// SPDX-License-Identifier: AGPL-3.0-or-later

import { brand } from '@app/brand'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  appUpdates,
  channel,
  channelChoice,
  downloadLine,
  isBigJump,
  isFix,
  updateLines,
  waitingForStable,
} from './appUpdate.svelte'
import { backend } from './backend'
import { prefs } from './prefs.svelte'

beforeEach(() => {
  prefs.channel = 'stable'
  prefs.channelAsked = false
})

describe('isBigJump', () => {
  it('two middle numbers or another first number is a big jump', () => {
    expect(isBigJump('0.6.0', '0.4.0')).toBe(true)
    expect(isBigJump('0.7.0', '0.4.2')).toBe(true)
    expect(isBigJump('0.6.0-beta.1', '0.4.0')).toBe(true)
    expect(isBigJump('1.0.0', '0.9.0')).toBe(true)
  })

  it('the next version or a fix is not', () => {
    expect(isBigJump('0.5.0', '0.4.3')).toBe(false)
    expect(isBigJump('0.5.0', '0.4.0-beta.2')).toBe(false)
    expect(isBigJump('0.4.1', '0.4.0')).toBe(false)
    expect(isBigJump('0.6.0', 'dev-abc')).toBe(false)
  })
})

describe('isFix', () => {
  it('only the last number changing is a fix', () => {
    expect(isFix('0.4.1', '0.4.0')).toBe(true)
    expect(isFix('v0.4.3', '0.4.1')).toBe(true)
  })

  it('another middle number or a beta is a new version', () => {
    expect(isFix('0.5.0', '0.4.2')).toBe(false)
    expect(isFix('1.4.0', '0.4.0')).toBe(false)
    expect(isFix('0.5.0-beta.1', '0.4.2')).toBe(false)
    expect(isFix('0.4.0-beta.3', '0.4.0-beta.2')).toBe(false)
    // the release a beta led to is new to its tester too
    expect(isFix('0.4.0', '0.4.0-beta.3')).toBe(false)
    expect(isFix('0.4.1', 'dev-abc')).toBe(false)
  })
})

describe('channels', () => {
  it('offers Nightly only where nightlies are built, Linux and Android', () => {
    // the tests run as the web version
    expect(channelChoice).toBe(false)
    prefs.channel = 'nightly'
    expect(channel()).toBe('stable')
  })

  it('a nightly or a beta on Stable waits for Stable to catch up', () => {
    expect(waitingForStable('nightly-2026-11-16')).toBe(true)
    expect(waitingForStable('nightly-2026-11-16.2')).toBe(true)
    expect(waitingForStable('0.4.0-beta.2')).toBe(true)
    expect(waitingForStable('0.6.0')).toBe(false)
  })
})

describe('updateLines', () => {
  const update = (messages?: string[]) => ({ version: '0.5.5', notes: 'Fixes.', messages })

  it('reads each message in the app language, filled in where it needs to be', () => {
    expect(updateLines(update(['faster', 'platform:Linux']))).toEqual([
      'Faster and lighter.',
      'Fixes for Linux.',
    ])
  })

  it('reads a message from a newer release as a new version, once', () => {
    expect(updateLines(update(['tomorrow', 'later', 'fixes']))).toEqual([
      'A new version is out.',
      'Fixes and small improvements.',
    ])
  })

  it('names a new language in the app language from its code', () => {
    expect(updateLines(update(['newLanguage:de']))).toEqual([
      `${brand.displayName} now speaks German.`,
    ])
  })

  it('has nothing to say for a release before the messages', () => {
    expect(updateLines(update())).toEqual([])
  })
})

describe('downloadLine', () => {
  it('says how far the download is, in megabytes', () => {
    expect(downloadLine({ done: 12_400_000, total: 24_200_000 })).toBe(
      'Downloading, 12.4 of 24.2 MB',
    )
    expect(downloadLine({ done: 3_000_000, total: null })).toBe('Downloading, 3 MB')
  })
})

describe('offered', () => {
  it('offers a version found once the notes are open', () => {
    appUpdates.ready = { version: '0.9.0', notes: '' }
    appUpdates.shown = false
    expect(appUpdates.offered).toBe(false)
    appUpdates.shown = true
    expect(appUpdates.offered).toBe(true)
  })

  it('offers it no more once put off, skipped or asked in the middle', () => {
    appUpdates.ready = { version: '0.9.0', notes: '' }
    appUpdates.shown = true
    appUpdates.dismissed = true
    expect(appUpdates.offered).toBe(false)
    appUpdates.dismissed = false
    appUpdates.skip()
    expect(appUpdates.offered).toBe(false)
    appUpdates.ready = { version: '0.9.1', notes: '' }
    expect(appUpdates.offered).toBe(true)
    appUpdates.critical = { version: '0.9.1', notes: '', critical: true }
    expect(appUpdates.offered).toBe(false)
  })

  it('skips the version with the cross, a security fix only until the next start', () => {
    appUpdates.critical = null
    appUpdates.dismissed = false
    appUpdates.shown = true
    appUpdates.ready = { version: '0.9.2', notes: '' }
    appUpdates.close()
    expect(appUpdates.skipped).toBe('0.9.2')
    expect(appUpdates.offered).toBe(false)
    appUpdates.ready = { version: '0.9.3', notes: '', critical: true }
    appUpdates.close()
    expect(appUpdates.skipped).toBe('0.9.2')
    expect(appUpdates.dismissed).toBe(true)
  })
})

describe('the Arch package', () => {
  beforeEach(() => {
    appUpdates.pacman = true
    appUpdates.ready = { version: '0.9.0', notes: '' }
    appUpdates.downloaded = true
    appUpdates.installed = false
    appUpdates.error = null
  })

  afterEach(() => {
    appUpdates.pacman = false
    vi.restoreAllMocks()
  })

  it('follows Stable only', () => {
    prefs.channel = 'nightly'
    expect(channel()).toBe('stable')
  })

  it('installs, then restarts on the next press', async () => {
    const install = vi.spyOn(backend, 'appUpdateInstall').mockResolvedValue()
    const restart = vi.spyOn(backend, 'appUpdateRestart').mockResolvedValue()
    await appUpdates.install()
    expect(install).toHaveBeenCalledOnce()
    expect(restart).not.toHaveBeenCalled()
    expect(appUpdates.installed).toBe(true)
    expect(appUpdates.installing).toBe(false)
    await appUpdates.install()
    expect(install).toHaveBeenCalledOnce()
    expect(restart).toHaveBeenCalledOnce()
  })

  it('keeps the card with the reason and a code when the password window is closed', async () => {
    vi.spyOn(backend, 'appUpdateInstall').mockRejectedValue({
      kind: 'passwordCancelled',
      message: 'the password window was closed',
    })
    await appUpdates.install()
    expect(appUpdates.installed).toBe(false)
    expect(appUpdates.installing).toBe(false)
    expect(appUpdates.downloaded).toBe(true)
    expect(appUpdates.error?.code).toBe('INSTALL-PASSWORD-CANCELLED')
    expect(appUpdates.error?.reason).toBe(
      'The password window was closed, so nothing was installed.',
    )
  })

  it("says pacman's own error", async () => {
    vi.spyOn(backend, 'appUpdateInstall').mockRejectedValue({
      kind: 'pacman',
      message: 'error: failed to init transaction (unable to lock database)',
    })
    await appUpdates.install()
    expect(appUpdates.error?.code).toBe('INSTALL-PACMAN')
    expect(appUpdates.error?.reason).toBe(
      'pacman could not install the update. It said “error: failed to init transaction (unable to lock database)”.',
    )
  })

  it('a newer version found starts over', () => {
    appUpdates.installed = true
    appUpdates.setChannel('nightly', false)
    expect(appUpdates.installed).toBe(false)
  })
})
