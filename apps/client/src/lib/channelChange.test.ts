// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest'
import type { AppUpdate } from './backend'
import { betaJoinsGently, leaveBeta, syncSafety } from './channelChange.svelte'

const stable = (o: Partial<AppUpdate> = {}): AppUpdate => ({
  version: '0.4.1',
  notes: '',
  older: true,
  sameSchema: true,
  goBack: true,
  ...o,
})

describe('syncSafety', () => {
  it('no sync server', () => {
    expect(syncSafety({ state: 'off', pending: 0 })).toBe('none')
    expect(syncSafety({ state: 'signedOut', pending: 0 })).toBe('none')
  })

  it('signed in and up to date', () => {
    expect(syncSafety({ state: 'synced', pending: 0 })).toBe('synced')
  })

  it('signed in with changes not yet on the server', () => {
    expect(syncSafety({ state: 'synced', pending: 2 })).toBe('uploading')
    expect(syncSafety({ state: 'pending', pending: 2 })).toBe('uploading')
    expect(syncSafety({ state: 'syncing', pending: 0 })).toBe('uploading')
    expect(syncSafety({ state: 'offline', pending: 0 })).toBe('uploading')
    expect(syncSafety({ state: 'error', pending: 0 })).toBe('uploading')
  })
})

describe('leaveBeta', () => {
  const base = { installedBeta: true, mac: false, android: false }

  it('a stable install just changes channel', () => {
    expect(leaveBeta({ ...base, installedBeta: false, found: stable() })).toBe('stable')
  })

  it('the Mac never goes back', () => {
    expect(leaveBeta({ ...base, mac: true, found: stable() })).toBe('stable')
  })

  it('Stable caught up: nothing older to go back to', () => {
    expect(leaveBeta({ ...base, found: stable({ older: false }) })).toBe('stable')
  })

  it('waits when the list was not read or this build cannot go back', () => {
    expect(leaveBeta({ ...base, found: null })).toBe('wait')
    expect(leaveBeta({ ...base, found: stable({ goBack: false }) })).toBe('wait')
  })

  it('waits for the next stable when the schema differs', () => {
    expect(leaveBeta({ ...base, found: stable({ sameSchema: false }) })).toBe('schema')
    expect(leaveBeta({ ...base, android: true, found: stable({ sameSchema: false }) })).toBe(
      'schema',
    )
  })

  it('goes back by installing on a computer, through an uninstall on Android', () => {
    expect(leaveBeta({ ...base, found: stable() })).toBe('back')
    expect(leaveBeta({ ...base, android: true, found: stable() })).toBe('backAndroid')
  })
})

describe('betaJoinsGently', () => {
  const beta = (o: Partial<AppUpdate> = {}): AppUpdate => ({
    version: '0.6.0-beta.3',
    notes: '',
    sameSchema: true,
    ...o,
  })

  it('joins without a backup when the beta reads the same notes and is one version ahead', () => {
    expect(betaJoinsGently(beta(), '0.5.5')).toBe(true)
    expect(betaJoinsGently(null, '0.5.5')).toBe(true)
  })

  it('asks for a backup when the notes database changes', () => {
    expect(betaJoinsGently(beta({ sameSchema: false }), '0.5.5')).toBe(false)
    expect(betaJoinsGently(beta({ sameSchema: undefined }), '0.5.5')).toBe(false)
  })

  it('asks for a backup over two versions or more', () => {
    expect(betaJoinsGently(beta(), '0.4.2')).toBe(false)
  })
})
