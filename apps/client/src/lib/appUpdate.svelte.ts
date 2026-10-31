// SPDX-License-Identifier: AGPL-3.0-or-later
// The app updating itself (commands/updates.rs). Release builds look once
// after opening and then every day; a newer version downloads quietly and a
// card offers to restart into it. On Android the system installer takes over
// from AppInstaller (MainActivity.kt) and asks the person. With automatic
// updates off, the app only reads the release list, to tell of a critical
// security fix. A critical one is asked in the middle of the screen, and
// never installs without a yes.

import { backend, type AppUpdate } from './backend'
import { prefs } from './prefs.svelte'
import { isBackendError } from './workspace.svelte'

declare global {
  interface Window {
    /** Set by MainActivity on Android. */
    AppInstaller?: { install(): boolean }
  }
}

const DAY_MS = 24 * 60 * 60 * 1000
/** The critical version the person chose not to install, asked no more. */
const REFUSED_KEY = 'criticalRefused'

function refused(version: string): boolean {
  try {
    return localStorage.getItem(REFUSED_KEY) === version
  } catch {
    return false
  }
}

/** Only numbered releases update themselves, dev and preview builds don't. */
export const updatesHere = /^\d+\.\d+\.\d+$/.test(__APP_VERSION__)

class AppUpdates {
  ready: AppUpdate | null = $state(null)
  /** The notes are open: the card waits while the app is locked. */
  shown = $state(false)
  /** The card was closed: it comes back on the next start. */
  dismissed = $state(false)
  checking = $state(false)
  /** The last check found nothing newer. */
  upToDate = $state(false)
  /** The error kind of the last check. */
  error = $state('')
  installing = $state(false)
  /** A critical security fix waiting for the person's answer. */
  critical: AppUpdate | null = $state(null)
  private timer: ReturnType<typeof setInterval> | undefined

  /** Looks now and every day after, once per launch. */
  start() {
    if (!updatesHere || this.timer) return
    void this.look()
    this.timer = setInterval(() => void this.look(), DAY_MS)
  }

  /** The daily look: the full check with automatic updates on, else the
   * release list alone, for a critical fix. */
  async look() {
    if (prefs.autoUpdates) return this.check()
    try {
      const latest = await backend.appUpdateLatest()
      if (latest?.critical && !refused(latest.version)) this.critical = latest
    } catch {
      // the next day's look tries again
    }
  }

  async check() {
    if (!updatesHere || this.checking) return
    this.checking = true
    this.error = ''
    try {
      this.ready = await backend.appUpdateCheck()
      this.upToDate = !this.ready
      if (this.ready?.critical && !refused(this.ready.version)) this.critical = this.ready
    } catch (err) {
      this.error = isBackendError(err) ? err.kind : 'unknown'
    } finally {
      this.checking = false
    }
  }

  async install() {
    if (!this.ready) return
    if (window.AppInstaller) {
      if (!window.AppInstaller.install()) this.error = 'notFound'
      return
    }
    // the portable exe opens the download page and keeps running
    this.installing = !this.ready.page
    try {
      await backend.appUpdateInstall()
    } catch (err) {
      this.error = isBackendError(err) ? err.kind : 'unknown'
      this.installing = false
    }
  }

  /** "Update now" on a critical fix: downloaded first when it is not yet. */
  async updateCritical() {
    if (!this.ready) await this.check()
    await this.install()
  }

  /** "Don't update", confirmed: not asked again for this version. */
  refuseCritical() {
    if (!this.critical) return
    try {
      localStorage.setItem(REFUSED_KEY, this.critical.version)
    } catch {
      // asked again on the next start
    }
    this.critical = null
  }
}

export const appUpdates = new AppUpdates()
