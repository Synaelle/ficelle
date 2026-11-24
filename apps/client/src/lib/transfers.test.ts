// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { setLocale } from './i18n.svelte'
import type { Moving } from './sync.svelte'
import { movingLines, movingOf } from './transfers'

const MB = 1024 * 1024

const file = (name: string, sending: boolean, done: number, total: number): Moving => ({
  sha256: name.padEnd(64, '0'),
  name,
  sending,
  done: done * MB,
  total: total * MB,
})

describe('movingLines', () => {
  it('adds up the files sent and received apart', async () => {
    await setLocale('en')
    const moving = [
      file('site-survey.mp4', true, 120, 300),
      file('invoice-march.pdf', true, 0, 150),
      file('kitchen-plan.dwg', false, 12, 340),
    ]
    expect(movingLines(moving)).toEqual([
      'Sending 2 files, 120 of 450 MB',
      'Receiving 1 file, 12 of 340 MB',
    ])
  })

  it('says nothing while no file moves', () => {
    expect(movingLines([])).toEqual([])
  })
})

describe('movingOf', () => {
  it('finds a file by its hash', () => {
    const moving = [file('site-survey.mp4', false, 1, 2)]
    expect(movingOf(moving, 'site-survey.mp4'.padEnd(64, '0'))?.name).toBe('site-survey.mp4')
    expect(movingOf(moving, 'b'.repeat(64))).toBeNull()
  })
})
