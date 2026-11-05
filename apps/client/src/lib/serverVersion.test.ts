// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest'
import { protocolGap, protocolRange, versionGap } from './serverVersion'

describe('versionGap', () => {
  it('counts only the first two numbers', () => {
    expect(versionGap('0.3.2', '0.3.1')).toBeNull()
    expect(versionGap('0.5.0-beta.2', '0.5.0-beta.1')).toBeNull()
    expect(versionGap('0.5.0-beta.2', '0.5.1')).toBeNull()
    expect(versionGap('0.5.0', 'v0.5.3')).toBeNull()
  })

  it('says which one is behind', () => {
    expect(versionGap('0.5.0', '0.4.9')).toBe('serverOlder')
    expect(versionGap('0.4.1', '0.5.0-beta.1')).toBe('serverNewer')
    expect(versionGap('1.0.0', '0.9.0')).toBe('serverOlder')
    expect(versionGap('0.10.0', '0.9.0')).toBe('serverOlder')
  })

  it('stays quiet about what it cannot read', () => {
    expect(versionGap('0.5.0', 'unknown')).toBeNull()
    expect(versionGap('', '0.5.0')).toBeNull()
    expect(versionGap('0.5.0', '5')).toBeNull()
  })
})

describe('protocolGap', () => {
  it('says which side needs an update', () => {
    expect(protocolGap(1, 1, 1)).toBeNull()
    expect(protocolGap(2, 1, 3)).toBeNull()
    expect(protocolGap(1, 2, 3)).toBe('updateApp')
    expect(protocolGap(4, 2, 3)).toBe('updateServer')
  })

  it('asks for nothing when the server did not say', () => {
    expect(protocolGap(1, undefined, undefined)).toBeNull()
  })
})

describe('protocolRange', () => {
  it('reads as one number or a range', () => {
    expect(protocolRange(1, 1)).toBe('1')
    expect(protocolRange(1, 3)).toBe('1–3')
    expect(protocolRange(undefined, undefined)).toBeNull()
  })
})
