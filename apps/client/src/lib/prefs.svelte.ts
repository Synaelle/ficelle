// SPDX-License-Identifier: AGPL-3.0-or-later
// Display preferences of this device. They are not secret and are needed
// before the vault is unlocked (theme of the unlock screen), so they live in
// localStorage instead of the encrypted database.

import type { ModePreference } from '@app/ui/theme'
import { DEFAULT_THEME_ID } from '@app/ui/themes'
import type { NoteSort } from './backend'
import type { FontChoice } from './fonts'
import { isLocale, type Locale } from './i18n.svelte'
import { DEFAULT_SHORTCUTS, type ActionId } from './shortcuts'
import type { UiSize } from './uiSize'

export type Layout = 'lite' | 'standard'

export interface Prefs {
  theme: string
  mode: ModePreference
  /** Dark mode on pure black, for OLED screens. */
  pureBlack: boolean
  /** Fonts for note text and code over the theme's, null keeps the theme's. */
  notesFont: FontChoice | null
  codeFont: FontChoice | null
  /** User accent as #rrggbb, null keeps the theme accent. */
  accent: string | null
  /** The system's accent color, over `accent`, where the system has one. */
  systemAccent: boolean
  layout: Layout
  /** null follows the system language. */
  language: Locale | null
  sort: NoteSort
  /** Overrides of the default key bindings. */
  shortcuts: Partial<Record<ActionId, string>>
  /** Without an account: the person turned the official catalog on. */
  catalogLocal: boolean
  /** Themes added from the instance: id to the file's hash. */
  instanceThemes: Record<string, string>
  /** Remember the vault key in the OS keychain. */
  rememberKey: boolean
  /** Size of text, buttons and spacing everywhere. */
  uiSize: UiSize
  /** OpenDyslexic everywhere, whatever the theme's fonts. */
  dyslexicFont: boolean
  /** Desktop shortcut that opens the quick note window from any app, null is off. */
  quickNoteShortcut: string | null
  /** Typing "- " or "1. " starts a list. */
  autoLists: boolean
  /** GitHub's `:name:` shortcodes become their emoji, typed or read. */
  emojiShortcodes: boolean
  /** How times read: as the system shows them, or always 24 or 12 hours. */
  clock: 'system' | '24' | '12'
  /** The open note stays first in the note list, whatever the sort. */
  openNoteFirst: boolean
  /** Newer versions download by themselves and a card offers them. Off,
   * only a critical security fix is told, in the middle of the screen. */
  autoUpdates: boolean
  /** The open note follows other devices' edits as they come in. */
  coediting: boolean
  /** Lock an encrypted vault after this many idle minutes, 0 never. */
  autoLockMinutes: number
  /** Files up to this many megabytes come with each sync, larger ones when opened. */
  autoFetchMb: number
  /** Pane widths in px, set by dragging the dividers. */
  sidebarWidth: number
  listWidth: number
  /** This install's random id for feedback, made on first use. */
  feedbackInstall: string
  /** Dev and preview builds: the folder picked for issue reports on a
   * computer, null for the default. Phones keep theirs in the app (DevReports.kt). */
  reportFolder: string | null
  /** Dev and preview builds: a report starts with a screenshot of the app. */
  reportScreenshot: boolean
}

export const DEFAULT_PREFS: Prefs = {
  theme: DEFAULT_THEME_ID,
  // dark whatever the system says, the look Ficelle is made for; System is a choice
  mode: 'dark',
  pureBlack: false,
  notesFont: null,
  codeFont: null,
  accent: null,
  systemAccent: false,
  layout: 'standard',
  language: null,
  sort: 'updatedDesc',
  shortcuts: {},
  catalogLocal: false,
  instanceThemes: {},
  rememberKey: true,
  uiSize: 'medium',
  dyslexicFont: false,
  quickNoteShortcut: null,
  autoLists: true,
  emojiShortcodes: true,
  clock: 'system',
  openNoteFirst: false,
  autoUpdates: true,
  coediting: true,
  autoLockMinutes: 15,
  autoFetchMb: 10,
  sidebarWidth: 220,
  listWidth: 320,
  feedbackInstall: '',
  reportFolder: null,
  reportScreenshot: true,
}

const STORAGE_KEY = 'prefs'

function load(): Prefs {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    const loaded: Prefs = raw
      ? { ...DEFAULT_PREFS, ...(JSON.parse(raw) as Partial<Prefs>) }
      : { ...DEFAULT_PREFS }
    // a language removed since it was chosen goes back to the system's, so English
    if (loaded.language !== null && !isLocale(loaded.language)) loaded.language = null
    return loaded
  } catch {
    // storage can be unavailable (private mode) or hold broken json
    return { ...DEFAULT_PREFS }
  }
}

export const prefs: Prefs = $state(load())

/** Saves after any change. Call once from the root component. */
export function persistPrefs() {
  $effect(() => {
    const snapshot = JSON.stringify(prefs)
    try {
      localStorage.setItem(STORAGE_KEY, snapshot)
    } catch {
      // nothing to do, the preference stays for this session
    }
  })
}

export function shortcutFor(action: ActionId): string {
  return prefs.shortcuts[action] ?? DEFAULT_SHORTCUTS[action]
}
