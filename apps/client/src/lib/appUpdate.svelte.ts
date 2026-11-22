// SPDX-License-Identifier: AGPL-3.0-or-later
// The app updating itself (commands/updates.rs). Release builds read the
// release list once after opening, every few hours, and when the app comes
// back on screen after a while away (a phone keeps it open for days). A
// newer version gets a card; it downloads only when the person asks, its
// progress in the card, then the card offers to restart into it. On Android
// the system installer takes over from AppInstaller (MainActivity.kt) and
// asks the person. A critical security fix is asked in the middle of the
// screen, unless that is turned off, and never installs without a yes.
//
// Two channels, Stable and Beta, each its own list (commands/updates.rs).
// An older version is never
// offered, so a beta moved to Stable waits for Stable to catch up.

import { brand } from '@app/brand'
import { listen } from '@tauri-apps/api/event'
import { backend, type AppUpdate, type UpdateChannel } from './backend'
import { NIGHTLY, pretendRelease, resetBetaWelcome } from './beta.svelte'
import { currentLocale, t } from './i18n.svelte'
import { ANDROID, LINUX, PLATFORM } from './platform'
import { prefs } from './prefs.svelte'
import { lastNotified, rememberNotified, worthNotifying } from './updateNotify'
import { overlay } from './overlay.svelte'
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
      /** The app store that installed this app, or last updated it, by name. */
      store?(): string
      /** The connection is metered: mobile data, a phone's hotspot. */
      metered?(): boolean
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

/** The app store that installed this APK, or last updated it (F-Droid, the
 * apps that read F-Droid repositories, Obtainium, Google Play), see
 * MainActivity.kt: it updates it. The APK F-Droid builds is F-Droid's. */
const store = ANDROID ? window.AppInstaller?.store?.() || (__APP_STORE__ ? 'F-Droid' : '') : ''

/** Only numbered releases of the apps have updates, betas included. Dev
 * and preview builds don't (unless they pretend, with nothing downloaded),
 * nor the web version, which is always the one served. A store or a package
 * manager may bring them instead, see `AppUpdates.elsewhere`. */
export const updatesHere =
  (PLATFORM !== 'web' &&
    (/^\d+\.\d+\.\d+(-beta\.\d+)?$/.test(thisVersion) || NIGHTLY.test(thisVersion))) ||
  !!pretend?.has('updates')

/** Stable or Nightly can be chosen: nightlies are built for Linux and
 * Android only. */
export const channelChoice = ANDROID || LINUX

/** System notifications can be turned on: not in the portable exe, which
 * has no installed identity for Windows to show them under. */
export const notificationsHere = updatesHere && !__APP_PORTABLE__

/** The list this device follows. */
export function channel(): UpdateChannel {
  return channelChoice ? prefs.channel : 'stable'
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

/** `next` skips versions: two middle numbers or more ahead (0.4 to 0.6),
 * or another first number. Much has changed in between, a backup is asked
 * for first. */
export function isBigJump(next: string, current: string = thisVersion): boolean {
  const n = parts(next)
  const c = parts(current)
  return !!n && !!c && (n[0] !== c[0] || n[1] - c[1] >= 2)
}

/** This version is a beta and Stable is chosen: nothing comes until a
 * stable version is newer, never a step back. */
export const waitingForStable = (current: string = thisVersion) =>
  channel() === 'stable' && (parts(current)?.[3] != null || NIGHTLY.test(current))

/** The phone is on mobile data, as Android tells it (the web view cannot). */
const onMobileData = () => ANDROID && (window.AppInstaller?.metered?.() ?? false)

class AppUpdates {
  /** The newer version found, downloaded or not. */
  ready: AppUpdate | null = $state(null)
  /** `ready` is downloaded and checked, waiting to be installed. */
  downloaded = $state(false)
  /** Bytes so far and the size, while it downloads. */
  progress: { done: number; total: number | null } | null = $state(null)
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
  /** What updates this app instead of itself: a store's name, `package` for
   * a Linux package, empty when the app updates itself. Then it never looks
   * on its own, and Settings only says where updates come from. */
  elsewhere = $state(store)
  private started = false
  private timer: ReturnType<typeof setInterval> | undefined
  private lookedAt = 0

  /** Looks now, every few hours after, and when the app comes back on screen
   * after a while, once per launch. */
  async start() {
    if (!updatesHere || this.started) return
    this.started = true
    if (LINUX && (await backend.appUpdatePageOnly())) this.elsewhere = 'package'
    if (this.elsewhere) return
    void this.look()
    this.timer = setInterval(() => void this.look(), EVERY_MS)
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible' && Date.now() - this.lookedAt > AWAY_MS)
        void this.look()
    })
  }

  /** Reads the release list, downloading nothing. */
  async look() {
    this.lookedAt = Date.now()
    // a pretend release looks for nothing on its own, nor one told not to
    if (pretendRelease || !prefs.updateChecks) return
    try {
      this.found(await backend.appUpdateLatest(channel()))
    } catch {
      // the next look tries again
    }
  }

  /** What the list offers: kept as downloaded when it is the same version. */
  private found(update: AppUpdate | null) {
    if (update?.version !== this.ready?.version) this.downloaded = false
    this.ready = update
    this.upToDate = !update
    if (!update) return
    if (update.critical && !refused(update.version)) this.critical = update
    void this.notify(update)
  }

  private get notifying() {
    return notificationsHere && prefs.updateNotifications
  }

  /** A system notification, when turned on and the version is worth one. */
  private async notify(update: AppUpdate) {
    const worth = worthNotifying(update, {
      on: this.notifying,
      inFront: document.hasFocus(),
      notified: lastNotified(),
      skipped: this.skipped,
    })
    if (!worth) return
    rememberNotified(update.version)
    try {
      await backend.notify(
        brand.displayName,
        update.critical
          ? t('appUpdate.criticalTitle')
          : t('appUpdate.out', { version: update.version }),
      )
    } catch {
      // the card in the app still says it
    }
  }

  /** Turning the notifications on or off. On asks the system first; false when
   * it refused, the setting stays off. */
  async setNotifications(on: boolean): Promise<boolean> {
    if (!on) {
      prefs.updateNotifications = false
      return true
    }
    let allowed = false
    try {
      allowed = await backend.notificationsAllowed()
    } catch {
      // as refused
    }
    prefs.updateNotifications = allowed
    return allowed
  }

  async check() {
    if (!updatesHere || this.elsewhere || this.checking) return
    if (pretendRelease) {
      this.error = 'testBuild'
      return
    }
    this.checking = true
    this.error = ''
    try {
      this.found(await backend.appUpdateLatest(channel()))
    } catch (err) {
      this.error = isBackendError(err) ? err.kind : 'unknown'
    } finally {
      this.checking = false
    }
  }

  /** Downloads the version found, its progress in `progress`. A download
   * cut short goes on from where it stopped on the next try. */
  async download() {
    if (!this.ready || this.progress) return
    // a phone on mobile data asks first, unless allowed in Settings
    if (onMobileData() && !prefs.updatesOnMobileData) {
      const yes = await overlay.ask(t('appUpdate.mobileDataAsk'), {
        confirmLabel: t('appUpdate.download'),
      })
      if (!yes) return
    }
    this.error = ''
    this.progress = { done: 0, total: null }
    const stop = await listen<{ done: number; total: number | null }>(
      'app-update-progress',
      (event) => (this.progress = event.payload),
    )
    try {
      const update = await backend.appUpdateCheck(channel())
      this.ready = update
      this.downloaded = !!update
    } catch (err) {
      this.error = isBackendError(err) ? err.kind : 'unknown'
    } finally {
      stop()
      this.progress = null
    }
  }

  /** Downloads first when it is not yet, then installs on the next press;
   * a release page opens at once. */
  async install() {
    if (!this.ready) return
    if (!this.ready.page && !this.downloaded) return this.download()
    if (ANDROID && window.AppInstaller) {
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
    if (next === 'nightly') resetBetaWelcome()
    this.ready = null
    this.downloaded = false
    this.critical = null
    this.upToDate = false
    this.error = ''
    if (look && updatesHere && this.timer) void this.look()
  }

  /** "Update now" on a critical fix: downloaded first when it is not yet,
   * then installed, the person having said yes. */
  async updateCritical() {
    if (!this.ready) await this.check()
    if (this.ready && !this.ready.page && !this.downloaded) await this.download()
    if (this.downloaded || this.ready?.page) await this.install()
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

/** What a release can say about itself, in every language the app has. */
const MESSAGES = [
  'fixes',
  'features',
  'major',
  'faster',
  'look',
  'dataFixes',
  'security',
  'updateAll',
  'sync',
  'phone',
  'platform',
  'accessibility',
  'languages',
  'newLanguage',
  'beta',
] as const

/** A release's messages in the app's language. Each is a key, or a key and
 * what fills it in (`platform:Linux`). A key this version does not know,
 * from a newer release, reads as "A new version is out", once. */
export function updateLines(update: AppUpdate): string[] {
  const lines = (update.messages ?? []).map((message) => {
    const [key = '', value = ''] = message.split(/:(.*)/s)
    if (!(MESSAGES as readonly string[]).includes(key)) return t('appUpdate.message.generic')
    // a new language comes as its code (de): named in the app's language
    const language =
      key === 'newLanguage'
        ? (new Intl.DisplayNames([currentLocale()], { type: 'language' }).of(value) ?? value)
        : value
    return t(`appUpdate.message.${key}`, { platform: value, language })
  })
  return lines.filter((line, i) => lines.indexOf(line) === i)
}

/** How far a download is, "Downloading, 12.4 of 24.2 MB", while it runs. */
export function downloadLine(progress: { done: number; total: number | null }): string {
  const locale = currentLocale()
  const number = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 })
  const megabytes = new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: 'megabyte',
    maximumFractionDigits: 1,
  })
  const done = progress.done / 1_000_000
  return progress.total
    ? t('appUpdate.downloading', {
        done: number.format(done),
        total: megabytes.format(progress.total / 1_000_000),
      })
    : t('appUpdate.downloadingSome', { done: megabytes.format(done) })
}

/** The English list of changes needs a word saying so in other languages. */
export const changelogLabelled = () => currentLocale() !== 'en'

export const appUpdates = new AppUpdates()
