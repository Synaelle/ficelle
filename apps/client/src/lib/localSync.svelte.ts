// SPDX-License-Identifier: AGPL-3.0-or-later
// Sync with the person's other devices on the same network, from the native
// shell, which finds them, pairs them and syncs on its own threads. This
// follows its status and passes on what the settings ask. A device uses
// either this or a server, never both. In a plain browser there is none.

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { SyncState } from './sync.svelte'

export type DeviceState =
  'syncing' | 'synced' | 'waiting' | 'away' | 'needsUpdate' | 'updateThis' | 'failed'

export interface LocalDevice {
  /** Hex of its key, how the commands name it. */
  key: string
  name: string
  platform: 'windows' | 'linux' | 'macos' | 'android' | string
  version: string
  lastSyncAt: number | null
  state: DeviceState
  /** The other version, or why it failed. */
  detail: string | null
  /** Seen but out of reach for a while: a firewall may be in the way. */
  blocked: boolean
}

export interface Pairing {
  /** Six digits, empty once it failed or paired. */
  code: string
  secondsLeft: number
  /** ip:port for the other device to type when it cannot find this one. */
  address: string | null
  failed: 'tooManyTries' | 'expired' | null
  /** Name of the device that typed the code. */
  paired: string | null
}

export interface LocalStatus {
  open: boolean
  name: string
  /** A server is linked here: local sync is off. */
  serverLinked: boolean
  running: boolean
  /** The port this device listens on, for a firewall rule. */
  port: number | null
  discoveryError: string | null
  pairing: Pairing | null
  devices: LocalDevice[]
}

/** A failed command: `kind` is stable, see the shell's error.rs. */
export interface LocalError {
  kind: string
  message: string
}

export function isLocalError(err: unknown): err is LocalError {
  return typeof err === 'object' && err !== null && 'kind' in err && 'message' in err
}

const EMPTY: LocalStatus = {
  open: false,
  name: '',
  serverLinked: false,
  running: false,
  port: null,
  discoveryError: null,
  pairing: null,
  devices: [],
}

const native = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

/** What the sidebar says: the state its icon draws, and the device it is about. */
export interface LocalSummary {
  state: SyncState
  key: 'synced' | 'syncing' | 'waiting' | 'needsUpdate' | 'updateThis'
  name: string
}

class LocalSync {
  readonly available = native
  status: LocalStatus = $state(EMPTY)
  private started = false

  /** Follows the shell. Called once the vault is open. */
  async start() {
    if (!native || this.started) return
    this.started = true
    await listen<LocalStatus>('local-sync-status', (event) => {
      this.status = event.payload
    })
    this.status = await invoke<LocalStatus>('local_sync_status')
    // a phone syncs while the app is in front, and again on coming back
    const visible = () =>
      void invoke('local_sync_foreground', { visible: document.visibilityState === 'visible' })
    document.addEventListener('visibilitychange', visible)
    visible()
  }

  /** The line for the sidebar, null without paired devices. */
  get summary(): LocalSummary | null {
    const devices = this.status.devices
    if (devices.length === 0) return null
    const first = (state: DeviceState) => devices.find((d) => d.state === state)
    const updateThis = first('updateThis')
    if (updateThis) return { state: 'error', key: 'updateThis', name: updateThis.name }
    const needs = first('needsUpdate')
    if (needs) return { state: 'error', key: 'needsUpdate', name: needs.name }
    const syncing = first('syncing')
    if (syncing) return { state: 'syncing', key: 'syncing', name: syncing.name }
    const waiting = first('waiting')
    if (waiting) return { state: 'pending', key: 'waiting', name: waiting.name }
    return { state: 'synced', key: 'synced', name: '' }
  }

  async refresh() {
    if (native) this.status = await invoke<LocalStatus>('local_sync_status')
  }

  async rename(name: string) {
    this.status = await invoke<LocalStatus>('local_sync_set_name', { name })
  }

  async pairStart() {
    this.status = await invoke<LocalStatus>('local_sync_pair_start')
  }

  async pairCancel() {
    this.status = await invoke<LocalStatus>('local_sync_pair_cancel')
  }

  /** Pairs with the device showing `code`. Its name. */
  async pairCode(code: string, address: string | null): Promise<string> {
    const name = await invoke<string>('local_sync_pair_code', { code, address })
    await this.refresh()
    return name
  }

  async forget(key: string) {
    this.status = await invoke<LocalStatus>('local_sync_forget', { key })
  }

  now() {
    return invoke('local_sync_now')
  }

  /** `granted`, `denied` or `prompt`. With `ask`, the phone asks the person. */
  permission(ask: boolean) {
    return invoke<string>('local_sync_permission', { ask })
  }
}

export const localSync = new LocalSync()
