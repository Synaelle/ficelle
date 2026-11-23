// SPDX-License-Identifier: AGPL-3.0-or-later
// This device's automatic backups: the native side writes the backup files
// (see commands/backups.rs), this keeps the schedule while the notes are open,
// says when a run fails, and warns when none went through for a while.

import { backend } from './backend'
import type { BackupSchedule, BackupsView } from './backend/types'
import { errorCode, errorDetail, shownError, type ShownError } from './errors'
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

/** Backup failures with a text of their own. */
const BACKUP_REASONS: Record<string, string> = {
  notFound: 'backups.notThere',
  wrongPassword: 'backups.wrongPassword',
  unprotected: 'backups.unprotected',
}

/** What went wrong with a backup, in the person's language when known,
 * else as the native side said it. */
export function backupError(err: unknown): ShownError {
  const shown = shownError(err, 'BACKUP')
  const key = BACKUP_REASONS[shown.kind]
  return { ...shown, reason: key ? t(key) : errorDetail(err) || shown.reason }
}

/** Attachments a backup left out, their file damaged on this device. */
export const damagedError = (names: string[]): ShownError => ({
  kind: 'damagedFile',
  reason: t('backup.damaged', { count: names.length, files: names.join(', ') }),
  code: errorCode('BACKUP', 'damagedFile'),
})

/** The time zone, for the backup file's name. */
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
        const failed = backupError(err)
        toasts.push(t('backups.title'), {
          ...failed,
          reason: t('backups.failed', { error: failed.reason }),
        })
      } else {
        throw err
      }
    }
  }

  /** The notes password changed: a backup now, when they are on, so one
   * opens with the new password. Older ones keep the password they had. */
  async runAfterPasswordChange() {
    await this.refresh()
    const config = this.view?.config
    if (config && PERIOD[config.schedule] !== null) await this.run({ scheduled: true })
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
if (__APP_DEV_TOOLS__) (window as { __backups?: Backups }).__backups = backups
