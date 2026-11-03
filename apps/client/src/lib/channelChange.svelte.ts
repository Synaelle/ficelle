// SPDX-License-Identifier: AGPL-3.0-or-later
// Changing channel (lib/ChannelSwitch.svelte draws each step).
//
// Joining Beta: betas may have bugs, so a backup is asked for first, then
// the Beta list is read at once and the newest beta downloads like any
// update.
//
// Leaving Beta while a beta is installed: Stable has no newer version yet.
// The person can wait for it, or go back to the newest stable at once after
// a warning and the same backup question. Going back needs the stable
// version to read this beta's database (the same schema), else it waits.
// Windows and the AppImage install it over the beta. Android installs no
// older version over a newer one: the checked APK goes to Downloads, the
// app is uninstalled, the stable one installed, and the notes come back from
// the sync server or the backup. The Mac follows Beta only.
//
// "Switch without a backup" is offered only when the notes are on a sync
// server and nothing waits to upload.
//
// A dev or preview build pretending to be a beta (Test tools) walks every
// step with a made-up stable version, and stops with a "test build" message
// where something would be checked, downloaded, copied or installed.

import { brand } from '@app/brand'
import { appUpdates, thisVersion } from './appUpdate.svelte'
import { backend, type AppUpdate, type UpdateChannel } from './backend'
import { isMacApp, pretendRelease } from './beta.svelte'
import { prefs } from './prefs.svelte'
import { sync, type SyncState } from './sync.svelte'
import { isBackendError } from './workspace.svelte'

/** Where the notes stand on the sync server. */
export type SyncSafety = 'none' | 'uploading' | 'synced'

/** `none`: no sync server. `synced`: signed in, nothing waiting to upload.
 * `uploading`: signed in with changes not yet there, or not reachable. */
export function syncSafety(status: { state: SyncState; pending: number }): SyncSafety {
  if (status.state === 'off' || status.state === 'signedOut') return 'none'
  return status.state === 'synced' && status.pending === 0 ? 'synced' : 'uploading'
}

/** What picking Stable leads to.
 * - `stable`: nothing to go back from, the channel simply changes
 * - `wait`: no way back here (or the list was not read), it waits for Stable
 * - `schema`: the stable version cannot read this beta's notes, it waits
 * - `back`: the warning, then installed over the beta
 * - `backAndroid`: the warning, then the uninstall steps */
export type LeaveBeta = 'stable' | 'wait' | 'schema' | 'back' | 'backAndroid'

export function leaveBeta(o: {
  installedBeta: boolean
  mac: boolean
  android: boolean
  /** The Stable list's version, read with older ones allowed. */
  found: AppUpdate | null
}): LeaveBeta {
  if (!o.installedBeta || o.mac) return 'stable'
  if (!o.found) return 'wait'
  if (!o.found.older) return 'stable'
  if (!o.found.goBack) return 'wait'
  if (!o.found.sameSchema) return 'schema'
  return o.android ? 'backAndroid' : 'back'
}

const installedBeta = /-beta\.\d+$/.test(thisVersion)

/** The stable version a pretend beta goes back to. */
const PRETEND_STABLE: AppUpdate = {
  version: '0.3.9',
  notes: '',
  older: true,
  sameSchema: true,
  goBack: true,
}

/** Before it was asked, a beta install follows Beta. */
if (installedBeta && !prefs.channelAsked) prefs.channel = 'beta'

export type Step =
  | 'join'
  | 'warn'
  | 'schema'
  | 'backup'
  | 'going'
  | 'android'
  | 'failed'
  /** A pretend beta: here something would have been downloaded. */
  | 'testBuild'

class ChannelSwitch {
  /** The step on screen, null when closed. */
  step: Step | null = $state(null)
  /** The stable version going back to. */
  target: AppUpdate | null = $state(null)
  /** Android: where the APK was saved, empty when it could not be. */
  saved = $state('')
  /** A backup was made on the way. */
  backedUp = $state(false)
  /** The error kind when going back failed. */
  error = $state('')

  get open() {
    return this.step !== null
  }

  /** The way every choice of channel goes. */
  async choose(next: UpdateChannel) {
    if (next === 'beta') {
      if (prefs.channel !== 'beta') this.show('join')
      else appUpdates.setChannel('beta')
      return
    }
    let found: AppUpdate | null = null
    if (installedBeta && !isMacApp) {
      try {
        found = pretendRelease ? PRETEND_STABLE : await backend.appUpdateLatest('stable', true)
      } catch {
        // not read: it waits, as without a way back
      }
    }
    const way = leaveBeta({ installedBeta, mac: isMacApp, android: !!window.AppInstaller, found })
    if (way === 'stable' || way === 'wait') return appUpdates.setChannel('stable')
    this.target = found
    this.show(way === 'schema' ? 'schema' : 'warn')
  }

  private show(step: Step) {
    this.backedUp = false
    this.error = ''
    this.saved = ''
    this.step = step
  }

  /** "Wait for Stable": Stable chosen, nothing installed backwards. */
  waitForStable() {
    this.step = null
    appUpdates.setChannel('stable')
  }

  /** "Switch anyway", after the warning: the backup question. */
  confirmBack() {
    this.step = 'backup'
  }

  /** The backup is made, or skipped: on to the chosen channel. */
  async proceed(backedUp: boolean) {
    this.backedUp = backedUp
    if (this.step === 'join') return this.join()
    return this.goBack()
  }

  private async join() {
    if (pretendRelease) {
      appUpdates.setChannel('beta', false)
      this.step = 'testBuild'
      return
    }
    this.step = null
    appUpdates.setChannel('beta')
    await appUpdates.check()
  }

  /** Downloads the stable version, older allowed, and installs it (the app
   * restarts) or, on Android, saves it to Downloads for the steps. The
   * channel changes only once it is downloaded. */
  private async goBack() {
    if (pretendRelease) {
      appUpdates.setChannel('stable', false)
      if (!window.AppInstaller) return void (this.step = 'testBuild')
      this.saved = `Download/${brand.displayName.replace(/ /g, '-')}-${PRETEND_STABLE.version}.apk`
      this.step = 'android'
      return
    }
    this.step = 'going'
    try {
      const update = await backend.appUpdateCheck('stable', true)
      if (!update) throw { kind: 'update', message: 'the stable version was not found' }
      this.target = update
      // set without a look: a regular look would drop the older version
      appUpdates.setChannel('stable', false)
      if (window.AppInstaller) {
        this.saved = window.AppInstaller.saveToDownloads?.(update.version) ?? ''
        this.step = 'android'
        return
      }
      await backend.appUpdateInstall()
    } catch (err) {
      this.error = isBackendError(err) ? err.kind : 'unknown'
      this.step = 'failed'
    }
  }

  close() {
    this.step = null
  }
}

export const channelSwitch = new ChannelSwitch()

/** How safe the notes are on the sync server right now. */
export const notesSynced = () => syncSafety(sync.status)
