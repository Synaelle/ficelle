// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { isIdle, nextCheck } from './autolock.svelte'

const MINUTE = 60_000

describe('autolock', () => {
  it('locks once the time has passed, never when set to never', () => {
    expect(isIdle(0, 15 * MINUTE - 1, 15)).toBe(false)
    expect(isIdle(0, 15 * MINUTE, 15)).toBe(true)
    expect(isIdle(0, 1000 * MINUTE, 0)).toBe(false)
  })

  it('looks at most a minute later, so a sleep cannot hold the lock back', () => {
    expect(nextCheck(0, 0, 15)).toBe(MINUTE)
    expect(nextCheck(0, 14 * MINUTE + 30_000, 15)).toBe(30_000)
    expect(nextCheck(0, 20 * MINUTE, 15)).toBe(0)
  })

  it('needs no timer when set to never', () => {
    expect(nextCheck(0, 0, 0)).toBeNull()
  })
})
