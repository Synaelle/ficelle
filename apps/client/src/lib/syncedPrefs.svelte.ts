// SPDX-License-Identifier: AGPL-3.0-or-later
// The choices that follow the person to every device. A change made here
// goes into the vault, which syncs it with the notes; one made on another
// device comes back from the vault and replaces the one here. Choices about
// this device (its language, sizes, lock, updates...) stay out.
import { backend } from './backend'
import { localSync } from './localSync.svelte'
import { ARROW_CHOICES, DEFAULT_PREFS, prefs, type Prefs } from './prefs.svelte'
import { sync } from './sync.svelte'

export const SYNCED = [
  'mode',
  'tagMark',
  'sort',
  'openNoteFirst',
  'autoLists',
  'emojiShortcodes',
  'arrow',
  'clock',
  'dyslexicFont',
  'coediting',
  'shortcuts',
  'spellcheck',
  'haptics',
] as const satisfies readonly (keyof Prefs)[]

type Synced = (typeof SYNCED)[number]

const isSynced = (key: string): key is Synced => (SYNCED as readonly string[]).includes(key)

/** The values this version knows for the choices that are one of a few. */
const CHOICES: Partial<Record<Synced, readonly string[]>> = {
  mode: ['dark', 'light'],
  tagMark: ['folder', 'hash'],
  sort: ['updatedDesc', 'createdDesc', 'titleAsc'],
  arrow: ARROW_CHOICES,
  clock: ['system', '24', '12'],
}

/** A value this version can use: the default's kind, and one it knows when
 * the choice is one of a few. A newer version may send others. */
function usable(key: Synced, value: unknown): boolean {
  if (value === null || typeof value !== typeof DEFAULT_PREFS[key]) return false
  const choices = CHOICES[key]
  return !choices || choices.includes(value as string)
}

/** What the vault last said for each choice: a value equal to it came from
 * there and is not sent back. */
const known: Partial<Record<Synced, string>> = {}

/** Takes the vault's choices in, those of every device. */
export async function takeSyncedPrefs() {
  const values = prefs as Record<Synced, unknown>
  for (const [key, raw] of await backend.syncedSettings()) {
    if (!isSynced(key)) continue
    const value: unknown = JSON.parse(raw)
    if (!usable(key, value)) continue
    known[key] = raw
    if (JSON.stringify(values[key]) !== raw) values[key] = value
  }
}

/** Sends each choice changed here while the notes are open. Choices made
 * before stay on this device until changed again, so a device never pushes
 * what it had over what the others chose. Call from a component. */
export function sendSyncedPrefs() {
  let seen: Partial<Record<Synced, string>> | null = null
  $effect(() => {
    const now: Partial<Record<Synced, string>> = {}
    for (const key of SYNCED) now[key] = JSON.stringify(prefs[key])
    if (seen) {
      for (const key of SYNCED) {
        const value = now[key]
        if (value !== undefined && value !== seen[key] && value !== known[key]) {
          known[key] = value
          void backend.setSyncedSetting(key, value)
        }
      }
    }
    seen = now
  })
}

/** Whether the choices travel anywhere: devices are paired or a server is
 * linked. */
export const choicesTravel = () =>
  localSync.status.devices.length > 0 || sync.status.state !== 'off'
