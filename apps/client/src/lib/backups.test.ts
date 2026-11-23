// SPDX-License-Identifier: AGPL-3.0-or-later
// When scheduled backups run and when the person is warned.

import { describe, expect, it } from 'vitest'
import type { BackupSchedule, BackupsView } from './backend/types'
import { damagedError, due, overdue } from './backups.svelte'
import { setLocale } from './i18n.svelte'

const HOUR = 3_600_000
const DAY = 24 * HOUR

function view(
  schedule: BackupSchedule | null,
  status: Partial<BackupsView['status']> = {},
  running = false,
): BackupsView {
  return {
    config: schedule ? { folder: 'D:/Backups', schedule, keep: 10 } : null,
    status: {
      lastRunAt: null,
      lastSuccessAt: null,
      lastError: null,
      lastErrorKind: null,
      files: 0,
      storedBytes: 0,
      damaged: [],
      ...status,
    },
    running,
  }
}

describe('scheduled backups', () => {
  const now = 100 * DAY

  it('run when a period passed since the last that went through', () => {
    expect(due(view('daily'), now)).toBe(true)
    expect(
      due(view('daily', { lastSuccessAt: now - 23 * HOUR, lastRunAt: now - 23 * HOUR }), now),
    ).toBe(false)
    expect(
      due(view('daily', { lastSuccessAt: now - 25 * HOUR, lastRunAt: now - 25 * HOUR }), now),
    ).toBe(true)
    expect(
      due(view('hourly', { lastSuccessAt: now - 61 * 60_000, lastRunAt: now - 61 * 60_000 }), now),
    ).toBe(true)
    expect(
      due(view('weekly', { lastSuccessAt: now - 6 * DAY, lastRunAt: now - 6 * DAY }), now),
    ).toBe(false)
  })

  it('never when off, not set up, or running', () => {
    expect(due(view('off'), now)).toBe(false)
    expect(due(view(null), now)).toBe(false)
    expect(due(null, now)).toBe(false)
    expect(due(view('daily', {}, true), now)).toBe(false)
  })

  it('after a failure, try again in half an hour, not at every look', () => {
    const failed = { lastSuccessAt: now - 2 * DAY, lastRunAt: now - 10 * 60_000, lastError: 'gone' }
    expect(due(view('daily', failed), now)).toBe(false)
    expect(due(view('daily', { ...failed, lastRunAt: now - 31 * 60_000 }), now)).toBe(true)
  })

  it('warn after two periods without one going through', () => {
    expect(overdue(view('daily', { lastSuccessAt: now - 30 * HOUR }), now)).toBe(false)
    expect(overdue(view('daily', { lastSuccessAt: now - 49 * HOUR }), now)).toBe(true)
    // never one through, though tried a day ago
    expect(overdue(view('daily', { lastRunAt: now - 25 * HOUR }), now)).toBe(true)
    // just set up, not tried yet
    expect(overdue(view('daily'), now)).toBe(false)
    expect(overdue(view('off', { lastSuccessAt: now - 300 * DAY }), now)).toBe(false)
  })
})

describe('damagedError', () => {
  it('names the attachments left out, with a code to send back', async () => {
    await setLocale('en')
    const shown = damagedError(['fence quote.pdf', 'garden.jpg'])
    expect(shown.code).toBe('BACKUP-DAMAGED-FILE')
    expect(shown.reason).toContain('2 attachments are damaged')
    expect(shown.reason).toContain('fence quote.pdf, garden.jpg')
  })
})
