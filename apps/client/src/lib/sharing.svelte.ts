// SPDX-License-Identifier: AGPL-3.0-or-later
// Sharing, from the native shell: the shared spaces in this vault, the
// share dialog's state, and the calls behind it. The server keeps who is
// in a space, the vault its notes, the sync thread the rest. In a plain
// browser there is no sharing, the list stays empty.

import { invoke } from '@tauri-apps/api/core'
import { backend, type Id } from './backend'
import { sync } from './sync.svelte'
import { workspace } from './workspace.svelte'

export type Role = 'owner' | 'edit' | 'read'

/** A shared space in this vault and what it holds at its top. */
export interface SharedEntry {
  id: string
  role: Role
  kind: 'standard' | 'e2ee'
  ownerName: string
  folders: Id[]
  notes: { id: Id; title: string }[]
}

/** Someone a space is shared with, or a group. */
export interface Member {
  userId?: string
  groupId?: string
  name: string
  displayName?: string
  role: Role
  publicKey?: string
  /** Of a person's key, to compare with them. */
  fingerprint?: string | null
}

export interface Person {
  userId: string
  username: string
  displayName: string
  kind: 'standard' | 'e2ee'
  publicKey?: string
  fingerprint: string | null
}

export interface Group {
  id: string
  name: string
  members: number
}

/** Someone the server lists for an e2ee space's keys whom the owner has not
 * let in from this device, or whose key is not the one kept. */
export interface Waiting {
  userId: string
  name: string
  publicKey: string
  /** Let in before with another key. */
  changed: boolean
  fingerprint: string | null
}

export interface MemberChoice {
  userId?: string
  groupId?: string
  role: 'edit' | 'read'
}

/** What the dialog is about: an item not shared yet, or a shared space. */
export type ShareTarget =
  { item: { kind: 'note' | 'folder'; id: Id }; name: string } | { space: string; name: string }

const native = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

class Sharing {
  list: SharedEntry[] = $state([])
  /** The share dialog, open when set. */
  target: ShareTarget | null = $state(null)

  /** Sharing needs an account on a server. */
  get available() {
    return native && sync.status.state !== 'off' && sync.status.state !== 'signedOut'
  }

  async refresh() {
    if (!native) return
    try {
      this.list = await invoke<SharedEntry[]>('share_list')
    } catch {
      // locked a moment ago, or the vault is closing: the last list stays
      // until the next refresh
    }
  }

  /** The shared space holding a note or folder, none for personal ones. */
  space(id: string | undefined | null): SharedEntry | undefined {
    return id ? this.list.find((s) => s.id === id) : undefined
  }

  /** Read only here: in a space shared with this account to read. */
  readOnly(spaceId: string | undefined | null) {
    return this.space(spaceId)?.role === 'read'
  }

  /** The space whose top this folder or note is, for its menu. */
  rootOf(id: Id): SharedEntry | undefined {
    return this.list.find((s) => s.folders.includes(id) || s.notes.some((n) => n.id === id))
  }

  open(target: ShareTarget) {
    this.target = target
  }

  close() {
    this.target = null
  }

  find(name: string): Promise<Person> {
    return invoke('share_find', { name })
  }

  groups(): Promise<Group[]> {
    return invoke('share_groups')
  }

  members(space: string): Promise<Member[]> {
    return invoke('share_members', { space })
  }

  /** Shares the item, returns its id in the shared space. */
  async create(item: { kind: 'note' | 'folder'; id: Id }, members: MemberChoice[]) {
    const id = await invoke<string>('share_create', { item, members })
    await this.after()
    if (item.kind === 'note' && workspace.current?.id === item.id) await workspace.open(id)
    if (
      item.kind === 'folder' &&
      workspace.scope.kind === 'folder' &&
      workspace.scope.id === item.id
    )
      await workspace.setScope({ kind: 'folder', id })
    return id
  }

  waiting(space: string): Promise<Waiting[]> {
    return invoke('share_waiting', { space })
  }

  /** Lets a waiting member in with the key whose fingerprint was shown. */
  allow(space: string, who: Waiting): Promise<void> {
    return invoke('share_allow', { space, userId: who.userId, publicKey: who.publicKey })
  }

  setMembers(space: string, members: MemberChoice[]): Promise<Member[]> {
    return invoke('share_set_members', { space, members })
  }

  async stop(space: string) {
    try {
      await invoke('share_stop', { space })
    } finally {
      // the notes came back even when the server could not be reached
      await this.after()
    }
  }

  async leave(space: string) {
    await invoke('share_leave', { space })
    await this.after()
  }

  /** Moving in or out of a space gives items new ids: what was open or
   * chosen may be gone. */
  private async after() {
    await this.refresh()
    await workspace.load()
    const scope = workspace.scope
    if (scope.kind === 'folder' && scope.id && !workspace.folders.some((f) => f.id === scope.id))
      await workspace.setScope({ kind: 'all' })
    const current = workspace.current
    if (current) {
      try {
        await backend.getNote(current.id)
      } catch {
        await workspace.open(null)
      }
    }
  }
}

export const sharing = new Sharing()
workspace.readOnly = (spaceId) => sharing.readOnly(spaceId)
