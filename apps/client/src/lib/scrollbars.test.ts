// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { onBar } from './scrollbars'

const box = { left: 100, top: 50, right: 420, bottom: 650 }
const none = { right: 0, bottom: 0 }

describe('onBar', () => {
  it('finds the vertical bar along the right edge', () => {
    const bars = { vertical: 12, horizontal: 0 }
    expect(onBar(box, bars, none, 415, 300)).toBe(true)
    expect(onBar(box, bars, none, 408, 300)).toBe(true)
    expect(onBar(box, bars, none, 407, 300)).toBe(false)
  })

  it('leaves the border out', () => {
    const bars = { vertical: 12, horizontal: 0 }
    expect(onBar(box, bars, { right: 1, bottom: 0 }, 420, 300)).toBe(false)
    expect(onBar(box, bars, { right: 1, bottom: 0 }, 419, 300)).toBe(true)
  })

  it('finds the horizontal bar along the bottom', () => {
    const bars = { vertical: 0, horizontal: 12 }
    expect(onBar(box, bars, none, 200, 645)).toBe(true)
    expect(onBar(box, bars, none, 200, 630)).toBe(false)
  })

  it('is never outside the element', () => {
    const bars = { vertical: 12, horizontal: 12 }
    expect(onBar(box, bars, none, 430, 300)).toBe(false)
    expect(onBar(box, bars, none, 415, 40)).toBe(false)
  })
})
