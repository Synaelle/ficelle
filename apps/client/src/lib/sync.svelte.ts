// SPDX-License-Identifier: AGPL-3.0-or-later
// Sync with a server, from the native shell. The sync itself runs on a
// thread there, this follows its status and asks it to sign in or out. In a
// plain browser (pnpm dev:web) there is no sync: the status stays off.

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { backend } from './backend'
import { localSync } from './localSync.svelte'
import { prefs } from './prefs.svelte'
import { takeSyncedPrefs } from './syncedPrefs.svelte'
import { workspace } from './workspace.svelte'
import { protocolRange } from './serverVersion'

/** Window event after files arrived from the server. */
export const FILES_CHANGED = 'files-changed'

export type SyncState = 'off' | 'signedOut' | 'syncing' | 'offline' | 'error' | 'pending' | 'synced'

export interface SyncStatus {
  state: SyncState
  server: string | null
  username: string | null
  e2ee: boolean
  /** Local changes not yet on the server. */
  pending: number
  lastSyncAt: number | null
  error: string | null
  /** Changes from the server that could not be read. */
  rejected: number
  /** Connected live, other devices' changes arrive within a second. */
  live: boolean
  /** The server's admin allows co-editing. */
  coediting: boolean
}

/** How long a server check is reused before the server is asked again. */
const CHECK_REUSE_MS = 60_000

export interface ServerCheck {
  address: string
  name: string
  version: string
  compatible: boolean
  options: { registration: string; accountKinds: string; sso: string | null } | null
  /** The sync protocol versions the server speaks. */
  minProtocol?: number
  maxProtocol?: number
  /** It signs apps in through its own page in the browser. */
  signInPage?: boolean
}

/** How a sign-in through the server's page in the browser ended. */
export interface BrowserEnded {
  status: SyncStatus | null
  error: SyncError | null
}

export type SecondFactor = 'totp' | 'recoveryCode' | 'passkey' | 'email'

export interface SignInAnswer {
  status: SyncStatus | null
  secondFactor: SecondFactor[] | null
}

/** A failed sign-in: `code` is the server's error code, `offline`, or one of ours. */
export interface SyncError {
  code: string
  message: string
  retryAfterSeconds?: number
}

export function isSyncError(err: unknown): err is SyncError {
  return typeof err === 'object' && err !== null && 'code' in err && 'message' in err
}

const OFF: SyncStatus = {
  state: 'off',
  server: null,
  username: null,
  e2ee: false,
  pending: 0,
  lastSyncAt: null,
  error: null,
  rejected: 0,
  live: false,
  coediting: true,
}

const native = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

class Sync {
  readonly available = native
  status: SyncStatus = $state(OFF)
  /** The last sign-in through the browser that ended, for the settings. */
  browserEnded: BrowserEnded | null = $state(null)
  private started = false

  /** Follows the sync thread. Called once the vault is open. */
  async start() {
    if (!native || this.started) return
    this.started = true
    // the person's own devices always follow each other; a server's
    // admin, and the person, can turn it off
    workspace.coediting = () =>
      localSync.status.devices.length > 0 ||
      (this.status.state !== 'off' && this.status.coediting && prefs.coediting)
    await listen<SyncStatus>('sync-status', (event) => {
      this.status = event.payload
    })
    await listen('sync-changed', () => {
      void workspace.remoteChanged()
      void takeSyncedPrefs()
    })
    // the browser answered a sign-in on this computer
    await listen<BrowserEnded>('sync-browser', (event) => this.browserDone(event.payload))
    // files that arrived: the open note shows them
    await listen('files-changed', () => window.dispatchEvent(new Event(FILES_CHANGED)))
    this.status = await invoke<SyncStatus>('sync_status')
    $effect.root(() => {
      $effect(() => void backend.setAutoFetch(prefs.autoFetchMb))
    })
    // coming back to the app is when other devices' changes are wanted:
    // pull then rather than at the next minute
    const back = () => {
      if (document.visibilityState === 'visible' && this.status.state !== 'off') void this.now()
    }
    window.addEventListener('focus', back)
    document.addEventListener('visibilitychange', back)
  }

  /** Recent checks by address: a server is asked again only after a
   * minute, however often the person presses Continue or opens About. */
  private checks = new Map<string, { at: number; answer: Promise<ServerCheck> }>()

  checkServer(address: string): Promise<ServerCheck> {
    const key = address.trim().toLowerCase()
    const recent = this.checks.get(key)
    if (recent && Date.now() - recent.at < CHECK_REUSE_MS) return recent.answer
    const answer = invoke<ServerCheck>('sync_check_server', { address })
    this.checks.set(key, { at: Date.now(), answer })
    // a failed check is not kept: the next try asks again
    answer.catch(() => this.checks.delete(key))
    return answer
  }

  async signIn(address: string, username: string, password: string): Promise<SignInAnswer> {
    const answer = await invoke<SignInAnswer>('sync_sign_in', { address, username, password })
    if (answer.status) this.status = answer.status
    return answer
  }

  /** Opens the server's sign-in page in the browser. `page` is what the
   * browser tab says once the app has the answer. Returns the page's
   * address, to offer when no browser came up. */
  async browserStart(address: string, page: { done: string; failed: string }): Promise<string> {
    this.browserEnded = null
    const url = await invoke<string>('sync_browser_start', { address, page })
    // a computer's app opened it already, a phone's page does it
    window.AppSystem?.openBrowser?.(url)
    return url
  }

  /** The page's answer on a phone, from the inbox. */
  async browserFinish(url: string) {
    try {
      this.browserDone({
        status: await invoke<SyncStatus>('sync_browser_finish', { url }),
        error: null,
      })
    } catch (err) {
      this.browserDone({
        status: null,
        error: isSyncError(err) ? err : { code: 'other', message: String(err) },
      })
    }
  }

  browserCancel() {
    return invoke('sync_browser_cancel')
  }

  private browserDone(ended: BrowserEnded) {
    if (ended.status) this.status = ended.status
    this.browserEnded = ended
  }

  /** Asks the server to email a code for the sign-in waiting. */
  async emailCode() {
    await invoke('sync_email_code')
  }

  async secondFactor(method: SecondFactor, code: string) {
    this.status = await invoke<SyncStatus>('sync_second_factor', { method, code })
  }

  now() {
    return invoke('sync_now')
  }

  async disconnect() {
    this.status = await invoke<SyncStatus>('sync_disconnect')
  }
}

export const sync = new Sync()

/** The linked server's sync protocol versions, `1` or `1–3`, for About.
 * `null` without a server or when it does not answer. */
export async function serverProtocol(): Promise<string | null> {
  const server = sync.status.server
  if (!server) return null
  try {
    const { minProtocol, maxProtocol } = await sync.checkServer(server)
    return protocolRange(minProtocol, maxProtocol)
  } catch {
    return null
  }
}
