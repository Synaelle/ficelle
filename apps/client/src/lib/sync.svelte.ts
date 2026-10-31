// SPDX-License-Identifier: AGPL-3.0-or-later
// Sync with a server, from the native shell. The sync itself runs on a
// thread there, this follows its status and asks it to sign in or out. In a
// plain browser (pnpm dev:web) there is no sync: the status stays off.

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { backend } from './backend'
import { prefs } from './prefs.svelte'
import { sharing } from './sharing.svelte'
import { workspace } from './workspace.svelte'
import { instance } from './instance.svelte'

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

/** Someone with a note of a shared space open. */
export interface Viewer {
  name: string
  displayName: string
  note: string
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
  /** Who has which note open, by shared space, while connected live. */
  presence: Record<string, Viewer[]> = $state({})
  private started = false

  /** The others with this note open, this account's own devices left out. */
  others(space: string, note: string): Viewer[] {
    if (!space || !this.status.live || !this.status.coediting) return []
    return (this.presence[space] ?? []).filter(
      (v) => v.note === note && v.name !== this.status.username,
    )
  }

  /** Follows the sync thread. Called once the vault is open. */
  async start() {
    if (!native || this.started) return
    this.started = true
    workspace.coediting = () =>
      this.status.state !== 'off' && this.status.coediting && prefs.coediting
    await listen<{ space: string; viewers: Viewer[] }>('sync-presence', (event) => {
      this.presence[event.payload.space] = event.payload.viewers
    })
    await listen<SyncStatus>('sync-status', (event) => {
      // signing in or out changes whose rules apply: asked at once
      const account = event.payload.state === 'off' || event.payload.state === 'signedOut'
      const before = this.status.state === 'off' || this.status.state === 'signedOut'
      void instance.refresh(account !== before)
      this.status = event.payload
      // what was heard before the socket went down is stale
      if (!event.payload.live) this.presence = {}
      // a round may have joined or left shared spaces
      if (event.payload.state === 'synced') void sharing.refresh()
    })
    await listen('sync-changed', () => {
      void workspace.remoteChanged()
      void sharing.refresh()
    })
    // files that arrived: the open note shows them
    await listen('files-changed', () => window.dispatchEvent(new Event(FILES_CHANGED)))
    this.status = await invoke<SyncStatus>('sync_status')
    $effect.root(() => {
      $effect(() => void backend.setAutoFetch(prefs.autoFetchMb))
    })
    void sharing.refresh()
    // coming back to the app is when other devices' changes are wanted:
    // pull then rather than at the next minute
    const back = () => {
      if (document.visibilityState === 'visible' && this.status.state !== 'off') void this.now()
    }
    window.addEventListener('focus', back)
    document.addEventListener('visibilitychange', back)
    // the others in a shared space see who has which of its notes open:
    // the open note while the app is in view, nothing otherwise
    const shown = $state({ visible: document.visibilityState === 'visible' })
    document.addEventListener(
      'visibilitychange',
      () => (shown.visible = document.visibilityState === 'visible'),
    )
    $effect.root(() => {
      $effect(() => {
        const note = workspace.current
        const shared = shown.visible && note && note.spaceId ? note : null
        void invoke('sync_viewing', {
          space: shared?.spaceId ?? null,
          note: shared?.id ?? null,
        })
      })
    })
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
    const { minProtocol: min, maxProtocol: max } = await sync.checkServer(server)
    if (min === undefined || max === undefined) return null
    return min === max ? String(min) : `${min}–${max}`
  } catch {
    return null
  }
}
