// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { worthNotifying } from './updateNotify'

const update = { version: '0.5.0', critical: false }
const state = { on: true, inFront: false, notified: null, skipped: null }

describe('update notifications', () => {
  it('tells of a new version when turned on and out of sight', () => {
    expect(worthNotifying(update, state)).toBe(true)
  })

  it('stays quiet when off, in front, or already told', () => {
    expect(worthNotifying(update, { ...state, on: false })).toBe(false)
    expect(worthNotifying(update, { ...state, inFront: true })).toBe(false)
    expect(worthNotifying(update, { ...state, notified: '0.5.0' })).toBe(false)
  })

  it('tells of the next version after an earlier one', () => {
    expect(worthNotifying(update, { ...state, notified: '0.4.0' })).toBe(true)
  })

  it('skips a skipped version, unless it is critical', () => {
    expect(worthNotifying(update, { ...state, skipped: '0.5.0' })).toBe(false)
    expect(worthNotifying({ ...update, critical: true }, { ...state, skipped: '0.5.0' })).toBe(true)
  })
})
