// SPDX-License-Identifier: AGPL-3.0-or-later
// Keyboard shortcuts. A binding is written like "Mod+Shift+K": Mod is Ctrl
// on Windows and Linux and Cmd on macOS. Users can rebind any action in
// Settings, the overrides are stored in prefs.

import { t } from './i18n.svelte'
import { WEB } from './platform'

export const isMac = typeof navigator !== 'undefined' && /mac|iphone|ipad/i.test(navigator.platform)

export const ACTIONS = [
  'newNote',
  'search',
  'pin',
  'trash',
  'back',
  'settings',
  'syncNow',
  'bold',
  'italic',
  'strike',
  'code',
  'link',
  'checklist',
  'deleteLine',
] as const

export type ActionId = (typeof ACTIONS)[number]

export const DEFAULT_SHORTCUTS: Record<ActionId, string> = {
  // a browser keeps Ctrl+N for a new window, a page never gets it
  newNote: WEB ? 'Alt+N' : 'Mod+N',
  search: 'Mod+F',
  pin: 'Mod+Shift+P',
  trash: 'Mod+Backspace',
  // Alt+Left moves by a word in a Mac's text, where back is Cmd+[
  back: isMac ? 'Mod+[' : 'Alt+ArrowLeft',
  settings: 'Mod+,',
  syncNow: 'Mod+S',
  bold: 'Mod+B',
  italic: 'Mod+I',
  strike: 'Mod+Shift+X',
  code: 'Mod+E',
  link: 'Mod+K',
  checklist: 'Mod+Shift+9',
  deleteLine: 'Mod+Shift+K',
}

/** The binding of a key event, in the same format as the settings. */
export function bindingOf(event: KeyboardEvent): string | null {
  const key = event.key
  if (['Control', 'Meta', 'Shift', 'Alt'].includes(key)) return null
  const parts: string[] = []
  if (isMac ? event.metaKey : event.ctrlKey) parts.push('Mod')
  if (event.altKey) parts.push('Alt')
  if (event.shiftKey) parts.push('Shift')
  // letters are stored upper case so Shift does not change the name
  parts.push(key.length === 1 ? key.toUpperCase() : key)
  return parts.join('+')
}

/** Key names as the keyboard of the app's language writes them. */
const KEY_NAMES: Record<string, () => string> = {
  Mod: () => t('keys.ctrl'),
  Shift: () => t('keys.shift'),
  Alt: () => t('keys.alt'),
  Backspace: () => t('keys.backspace'),
  Delete: () => t('keys.delete'),
  Enter: () => t('keys.enter'),
  Escape: () => t('keys.escape'),
  Space: () => t('keys.space'),
  ArrowLeft: () => '←',
}

/** Human form of a binding for this platform, for example ⌘N or Ctrl+N. */
export function displayBinding(binding: string): string {
  if (!isMac)
    return binding
      .split('+')
      .map((part) => KEY_NAMES[part]?.() ?? part)
      .join('+')
  const symbols: Record<string, string> = {
    Mod: '⌘',
    Shift: '⇧',
    Alt: '⌥',
    Backspace: '⌫',
    ArrowLeft: '←',
  }
  return binding
    .split('+')
    .map((part) => symbols[part] ?? part)
    .join('')
}
