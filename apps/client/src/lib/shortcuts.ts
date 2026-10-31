// SPDX-License-Identifier: AGPL-3.0-or-later
// Keyboard shortcuts. A binding is written like "Mod+Shift+K": Mod is Ctrl
// on Windows and Linux and Cmd on macOS. Users can rebind any action in
// Settings, the overrides are stored in prefs.

export const ACTIONS = [
  'newNote',
  'search',
  'toggleSidebar',
  'toggleLayout',
  'pin',
  'trash',
  'settings',
  'bold',
  'italic',
  'strike',
  'code',
  'link',
  'heading',
  'checklist',
] as const

export type ActionId = (typeof ACTIONS)[number]

export const DEFAULT_SHORTCUTS: Record<ActionId, string> = {
  newNote: 'Mod+N',
  search: 'Mod+K',
  toggleSidebar: 'Mod+\\',
  toggleLayout: 'Mod+Shift+L',
  pin: 'Mod+Shift+P',
  trash: 'Mod+Backspace',
  settings: 'Mod+,',
  bold: 'Mod+B',
  italic: 'Mod+I',
  strike: 'Mod+Shift+X',
  code: 'Mod+E',
  link: 'Mod+L',
  heading: 'Mod+Alt+1',
  checklist: 'Mod+Shift+9',
}

export const isMac = typeof navigator !== 'undefined' && /mac|iphone|ipad/i.test(navigator.platform)

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

/** Human form of a binding for this platform, for example ⌘N or Ctrl+N. */
export function displayBinding(binding: string): string {
  if (!isMac) return binding.replace('Mod', 'Ctrl')
  const symbols: Record<string, string> = { Mod: '⌘', Shift: '⇧', Alt: '⌥', Backspace: '⌫' }
  return binding
    .split('+')
    .map((part) => symbols[part] ?? part)
    .join('')
}
