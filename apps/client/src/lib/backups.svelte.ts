// SPDX-License-Identifier: AGPL-3.0-or-later
// This device's scheduled backups: the native side does the work (see
// commands/backups.rs), this keeps the schedule while the notes are open,
// says when a run fails, and warns when none went through for a while.

import { backend } from './backend'
import type { BackupSchedule, BackupsView } from './backend/types'
import { t } from './i18n.svelte'
import { toasts } from './toast.svelte'

const HOUR = 3_600_000
const PERIOD: Record<BackupSchedule, number | null> = {
  off: null,
  hourly: HOUR,
  daily: 24 * HOUR,
  weekly: 7 * 24 * HOUR,
}
/** A failed run is tried again after this, whatever the schedule. */
const RETRY = 30 * 60_000
/** How often the schedule is looked at. */
const TICK = 5 * 60_000
const WARNED = 'backup-overdue-warned'

/** Whether a scheduled run is due at `now`. */
export function due(view: BackupsView | null, now: number): boolean {
  const period = view?.config ? PERIOD[view.config.schedule] : null
  if (!view || period === null || view.running) return false
  const { lastSuccessAt, lastRunAt } = view.status
  const sinceSuccess = lastSuccessAt === null ? Infinity : now - lastSuccessAt
  const sinceRun = lastRunAt === null ? Infinity : now - lastRunAt
  return sinceSuccess >= period && sinceRun >= Math.min(period, RETRY)
}

/** Whether no run went through for two periods (or since set up, a run
 * having been tried): worth telling the person. */
export function overdue(view: BackupsView | null, now: number): boolean {
  const period = view?.config ? PERIOD[view.config.schedule] : null
  if (!view || period === null) return false
  const { lastSuccessAt, lastRunAt } = view.status
  if (lastSuccessAt === null) return lastRunAt !== null && now - lastRunAt >= period
  return now - lastSuccessAt >= 2 * period
}

/** What went wrong with a backup, in the person's language when known. */
export function backupError(kind: string | null | undefined, message: string): string {
  if (kind === 'notFound') return t('backups.notThere')
  if (kind === 'wrongPassword') return t('backups.wrongPassword')
  return message
}

/** The time zone, for the policy's days. */
const offsetSeconds = () => -new Date().getTimezoneOffset() * 60

class Backups {
  view: BackupsView | null = $state(null)
  private timer: ReturnType<typeof setInterval> | null = null

  async refresh() {
    if (!backend.native) return
    try {
      this.view = await backend.backupsGet()
    } catch {
      // locked or not ready: the next look sees it
    }
  }

  /** Backs up now. `scheduled`: a failure is told in a message, a run
   * asked for says it where it was asked. */
  async run({ scheduled = false } = {}) {
    if (this.view) this.view = { ...this.view, running: true }
    try {
      this.view = await backend.backupsRun(offsetSeconds())
    } catch (err) {
      await this.refresh()
      if (scheduled) {
        const e = err as { kind?: string; message?: string }
        const message = backupError(e.kind, e.message ?? String(err))
        toasts.push(t('backups.title'), t('backups.failed', { error: message }))
      } else {
        throw err
      }
    }
  }

  /** Looks at the schedule now and every few minutes, while the notes are open. */
  start() {
    this.stop()
    const look = async () => {
      await this.refresh()
      const now = Date.now()
      if (due(this.view, now)) await this.run({ scheduled: true })
      if (overdue(this.view, now)) this.warnOnce(now)
    }
    // not at once: the notes open first
    const first = setTimeout(() => void look(), 20_000)
    this.timer = setInterval(() => void look(), TICK)
    this.stopFirst = () => clearTimeout(first)
  }

  private stopFirst: () => void = () => {}

  stop() {
    this.stopFirst()
    if (this.timer) clearInterval(this.timer)
    this.timer = null
  }

  /** Once a day at most. */
  private warnOnce(now: number) {
    try {
      const last = Number(localStorage.getItem(WARNED) ?? 0)
      if (now - last < 24 * HOUR) return
      localStorage.setItem(WARNED, String(now))
    } catch {
      // without storage the warning shows each time, still rare
    }
    toasts.push(t('backups.title'), t('backups.overdue'))
  }
}

export const backups = new Backups()

// dev builds let the end to end tests look at it
if (__APP_DEV_REPORTS__) (window as { __backups?: Backups }).__backups = backups
