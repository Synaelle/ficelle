// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it } from 'vitest'
import { channel, channelAskDue, isBigJump, isFix, waitingForStable } from './appUpdate.svelte'
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
  it('follows the chosen list', () => {
    expect(channel()).toBe('stable')
    prefs.channel = 'beta'
    expect(channel()).toBe('beta')
  })

  it('a beta on Stable waits for Stable to catch up', () => {
    expect(waitingForStable('0.4.0-beta.2')).toBe(true)
    expect(waitingForStable('0.4.0')).toBe(false)
    prefs.channel = 'beta'
    expect(waitingForStable('0.4.0-beta.2')).toBe(false)
  })

  it('asks once, from 0.4.0 on', () => {
    expect(channelAskDue('0.3.2')).toBe(false)
    expect(channelAskDue('dev-abc')).toBe(false)
    expect(channelAskDue('0.4.0-beta.1')).toBe(true)
    expect(channelAskDue('0.4.0')).toBe(true)
    expect(channelAskDue('1.0.0')).toBe(true)
    prefs.channelAsked = true
    expect(channelAskDue('0.4.0')).toBe(false)
  })
})
