// SPDX-License-Identifier: AGPL-3.0-or-later
// The app updating itself (commands/updates.rs). Release builds look once
// after opening, every few hours, and when the app comes back on screen
// after a while away (a phone keeps it open for days); a newer version
// downloads quietly and a
// card offers to restart into it. On Android the system installer takes over
// from AppInstaller (MainActivity.kt) and asks the person. With automatic
// updates off, the app only reads the release list, to tell of a critical
// security fix, unless that is turned off too. A critical one is asked in the middle of the screen, and
// never installs without a yes.
//
// Two channels, Stable and Beta, each its own list (commands/updates.rs).
// The Mac app follows Beta, with no choice. An older version is never
// offered, so a beta moved to Stable waits for Stable to catch up.

import { backend, type AppUpdate, type UpdateChannel } from './backend'
import { isMacApp, pretendRelease, resetBetaWelcome } from './beta.svelte'
import { prefs } from './prefs.svelte'
import { isBackendError } from './workspace.svelte'

declare global {
  interface Window {
    /** Set by MainActivity on Android. */
    AppInstaller?: {
      install(): boolean
      /** Copies the checked APK of `version` to Downloads, returns where. */
      saveToDownloads?(version: string): string
      /** Opens the system's screen to uninstall this app. */
      uninstall?(): boolean
      /** Backups stay when the app is uninstalled (Android 11 and later). */
      backupsKept?(): boolean
    }
  }
}

const HOUR_MS = 60 * 60 * 1000
/** Between two looks while the app stays open. */
const EVERY_MS = 6 * HOUR_MS
/** Back on screen after this long away, it looks again. */
const AWAY_MS = HOUR_MS
/** The critical version the person chose not to install, asked no more. */
const REFUSED_KEY = 'criticalRefused'
/** The version whose card the person skipped, shown again for the next. */
const SKIPPED_KEY = 'updateSkipped'

function skippedVersion(): string | null {
  try {
    return localStorage.getItem(SKIPPED_KEY)
  } catch {
    return null
  }
}

function refused(version: string): boolean {
  try {
    return localStorage.getItem(REFUSED_KEY) === version
  } catch {
    return false
  }
}

/** The browser dev server, to see the settings: ?updates shows them,
 * ?channelAsk asks for the channel. */
const pretend = import.meta.env.DEV ? new URLSearchParams(location.search) : null

/** This build's version, a beta's when Test tools pretend it is one. */
export const thisVersion = pretendRelease ? '0.4.0-beta.1' : __APP_VERSION__

/** Only numbered releases update themselves, betas included, dev and
 * preview builds don't (unless they pretend, with nothing downloaded). */
export const updatesHere =
  /^\d+\.\d+\.\d+(-beta\.\d+)?$/.test(thisVersion) || !!pretend?.has('updates')

/** Stable or Beta can be chosen: everywhere but the Mac, always on Beta. */
export const channelChoice = !isMacApp

/** The list this device follows. */
export function channel(): UpdateChannel {
  return channelChoice ? prefs.channel : 'beta'
}

/** `major.minor.micro` and a beta's number, null for anything else. */
function parts(version: string): [number, number, number, number | null] | null {
  const match = /^v?(\d+)\.(\d+)\.(\d+)(?:-beta\.(\d+))?$/.exec(version.trim())
  if (!match) return null
  return [Number(match[1]), Number(match[2]), Number(match[3]), match[4] ? Number(match[4]) : null]
}

/** `next` only fixes `current`: the same `major.minor`, neither a beta. A
 * changed middle number, or a beta, is a new version. */
export function isFix(next: string, current: string = thisVersion): boolean {
  const n = parts(next)
  const c = parts(current)
  return !!n && !!c && n[0] === c[0] && n[1] === c[1] && n[3] === null && c[3] === null
}

/** This version is a beta and Stable is chosen: nothing comes until a
 * stable version is newer, never a step back. */
export const waitingForStable = (current: string = thisVersion) =>
  channel() === 'stable' && parts(current)?.[3] != null

/** Asked once which channel to follow, from 0.4.0 on. */
export function channelAskDue(current: string = thisVersion): boolean {
  if (!channelChoice || prefs.channelAsked) return false
  if (pretend?.has('channelAsk')) return true
  const p = parts(current)
  return !!p && (p[0] > 0 || p[1] >= 4)
}

class AppUpdates {
  ready: AppUpdate | null = $state(null)
  /** The notes are open: the card waits while the app is locked. */
  shown = $state(false)
  /** The card was closed: it comes back on the next start. */
  dismissed = $state(false)
  /** The version whose card was skipped: no card until a newer one. */
  skipped = $state(skippedVersion())
  checking = $state(false)
  /** The last check found nothing newer. */
  upToDate = $state(false)
  /** The error kind of the last check. */
  error = $state('')
  installing = $state(false)
  /** A critical security fix waiting for the person's answer. */
  critical: AppUpdate | null = $state(null)
  private timer: ReturnType<typeof setInterval> | undefined
  private lookedAt = 0

  /** Looks now, every few hours after, and when the app comes back on screen
   * after a while, once per launch. */
  start() {
    if (!updatesHere || this.timer) return
    void this.look()
    this.timer = setInterval(() => void this.look(), EVERY_MS)
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible' && Date.now() - this.lookedAt > AWAY_MS)
        void this.look()
    })
  }

  /** The full check with automatic updates on, else the release list alone,
   * for a critical fix. */
  async look() {
    this.lookedAt = Date.now()
    // a pretend release looks for nothing on its own
    if (pretendRelease) return
    if (prefs.autoUpdates) return this.check()
    // both turned off: nothing is asked at all
    if (!prefs.securityChecks) return
    try {
      const latest = await backend.appUpdateLatest(channel())
      if (latest?.critical && !refused(latest.version)) this.critical = latest
    } catch {
      // the next look tries again
    }
  }

  async check() {
    if (!updatesHere || this.checking) return
    if (pretendRelease) {
      this.error = 'testBuild'
      return
    }
    this.checking = true
    this.error = ''
    try {
      this.ready = await backend.appUpdateCheck(channel())
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

  /** The person chose a channel. Another one than before: what the other
   * list offered goes, and the new list is read. A beta found on Beta is
   * never offered on Stable. */
  setChannel(next: UpdateChannel, look = true) {
    prefs.channelAsked = true
    if (prefs.channel === next) return
    prefs.channel = next
    // the beta welcome greets someone joining Beta, again if they left it
    if (next === 'beta') resetBetaWelcome()
    this.ready = null
    this.critical = null
    this.upToDate = false
    this.error = ''
    if (look && updatesHere && this.timer) void this.look()
  }

  /** "Update now" on a critical fix: downloaded first when it is not yet. */
  async updateCritical() {
    if (!this.ready) await this.check()
    await this.install()
  }

  /** "Skip this version": its card comes back no more, the next version's
   * does. Never for a critical fix, and Settings still offers it. */
  skip() {
    if (!this.ready || this.ready.critical) return
    this.skipped = this.ready.version
    try {
      localStorage.setItem(SKIPPED_KEY, this.ready.version)
    } catch {
      // only this run forgets it
    }
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
