// SPDX-License-Identifier: AGPL-3.0-or-later
// Applies the chosen theme, light or dark mode and accent to the page as css
// variables, plus the theme's own css. Mode follows the system unless the
// user picked one, dark when the system has no preference. Themes the person
// imported are kept by the app beside the vault (not in it: the lock screen
// is themed too), a theme being looked at in the picker shows on the whole
// app until the pointer leaves it.

import {
  resolveMode,
  themeToCssVars,
  validateTheme,
  withAccent,
  type Mode,
  type Theme,
} from '@app/ui/theme'
import { builtInThemes, DEFAULT_THEME_ID } from '@app/ui/themes'
import { familyOf, loadFontPack, loadListedFont, quoted, themeFontPack } from './fonts'
import { prefs } from './prefs.svelte'
import { readSystemAccent, systemAccent } from './systemAccent.svelte'
import { clock, readSystemClock } from './clock.svelte'
import { backend } from './backend'

const darkQuery = '(prefers-color-scheme: dark)'
const lightQuery = '(prefers-color-scheme: light)'
// the chosen imported theme, so the first paint has it before the list loads
const ACTIVE_KEY = 'theme-active'

function systemPrefersLight(): boolean | null {
  if (matchMedia(lightQuery).matches) return true
  if (matchMedia(darkQuery).matches) return false
  return null
}

function cachedActive(): Theme[] {
  try {
    const theme = JSON.parse(localStorage.getItem(ACTIVE_KEY) ?? 'null') as unknown
    return validateTheme(theme).length === 0 ? [theme as Theme] : []
  } catch {
    return []
  }
}

const system = $state({ prefersLight: systemPrefersLight() })
const state = $state({
  imported: cachedActive(),
  /** A theme shown while the person looks at it, the chosen one stays in prefs. */
})

// loaded when needed: the admin page styles itself without the app's storage
const store = async () => (await import('./backend')).backend

// which imported themes the rules of the instance allow, set by the app
// (lib/instance), the admin page has none
const filter = $state<{ allowed: (id: string) => boolean }>({ allowed: () => true })

/** Sets which imported themes may be used. */
export function setThemeFilter(allowed: (id: string) => boolean) {
  filter.allowed = allowed
}

const builtInIds = new Set(builtInThemes.map((t) => t.id))

export const isBuiltIn = (id: string) => builtInIds.has(id)

/** Built-in themes first, then the imported ones the rules allow, by name. */
export function allThemes(): Theme[] {
  return [...builtInThemes, ...state.imported.filter((t) => filter.allowed(t.id))]
}

// Always the default theme for now: choosing another, imported ones too,
// comes back with the themes page. A theme saved by an older version is
// left in the prefs and simply not used.
function chosenTheme(): Theme {
  return builtInThemes.find((t) => t.id === DEFAULT_THEME_ID) ?? (builtInThemes[0] as Theme)
}

export function currentTheme(): Theme {
  return chosenTheme()
}

export function currentMode(): Mode {
  return resolveMode(prefs.mode, system.prefersLight)
}

/** Reads the imported themes again, leaving out any that no longer passes the checks. */
export async function loadImportedThemes() {
  const texts = await (await store()).themesList()
  state.imported = texts
    .map((text) => {
      try {
        return JSON.parse(text) as unknown
      } catch {
        return null
      }
    })
    .filter((t): t is Theme => validateTheme(t).length === 0 && !isBuiltIn((t as Theme).id))
    .sort((a, b) => a.name.localeCompare(b.name))
}

export type ImportResult =
  | { ok: true; theme: Theme; replaced: boolean }
  | { ok: false; kind: 'notJson' | 'builtIn' | 'invalid'; problems: string[] }

/** Checks a theme file and keeps it. The person still chooses it. */
export async function importTheme(text: string): Promise<ImportResult> {
  let theme: unknown
  try {
    theme = JSON.parse(text)
  } catch (err) {
    return { ok: false, kind: 'notJson', problems: [String(err)] }
  }
  const problems = validateTheme(theme)
  if (problems.length > 0) return { ok: false, kind: 'invalid', problems }
  const valid = theme as Theme
  if (isBuiltIn(valid.id)) return { ok: false, kind: 'builtIn', problems: [] }
  const replaced = state.imported.some((t) => t.id === valid.id)
  await (await store()).themeSave(text)
  await loadImportedThemes()
  if (prefs.theme === valid.id) remember(valid)
  return { ok: true, theme: valid, replaced }
}

export async function removeTheme(id: string) {
  if (isBuiltIn(id)) return
  if (prefs.theme === id) prefs.theme = DEFAULT_THEME_ID
  await (await store()).themeRemove(id)
  await loadImportedThemes()
}

/**
 * The theme on screen as a file: with the person's accent when they chose
 * one, under a new id when it is a built-in one, which an import would
 * refuse.
 */
export function exportTheme(copyName: (name: string) => string): { id: string; json: string } {
  const theme = chosenTheme()
  const copy = isBuiltIn(theme.id) || prefs.accent !== null
  const accent = prefs.accent ?? undefined
  const out: Theme = {
    ...theme,
    id: copy ? `${theme.id}-copy`.slice(0, 40) : theme.id,
    name: copy ? copyName(theme.name).slice(0, 60) : theme.name,
    colors: {
      dark: withAccent(theme.colors.dark, accent),
      light: withAccent(theme.colors.light, accent),
    },
    version: 2,
  }
  return { id: out.id, json: JSON.stringify(out, null, 2) + '\n' }
}

function remember(theme: Theme | null) {
  try {
    if (theme) localStorage.setItem(ACTIVE_KEY, JSON.stringify(theme))
    else localStorage.removeItem(ACTIVE_KEY)
  } catch {
    // storage can be unavailable, the theme still loads from the list
  }
}

/**
 * Keeps the page styled. Call once from the root component. The admin page
 * has no imported themes and carries every built-in theme's fonts, it
 * downloads none.
 */
export function applyTheme({
  imported = true,
  downloads = true,
}: { imported?: boolean; downloads?: boolean } = {}) {
  $effect(() => {
    const update = () => (system.prefersLight = systemPrefersLight())
    const queries = [matchMedia(darkQuery), matchMedia(lightQuery)]
    queries.forEach((q) => q.addEventListener('change', update))
    return () => queries.forEach((q) => q.removeEventListener('change', update))
  })

  $effect(() => {
    // failing to list leaves the built-in themes, the cached one included
    if (imported) loadImportedThemes().catch(() => {})
  })

  // a listed font chosen on this device is loaded from its files; one that
  // went missing is downloaded again
  $effect(() => {
    if (!downloads) return
    for (const choice of [prefs.notesFont, prefs.codeFont])
      if (choice?.source === 'list') void loadListedFont(choice.id, true).catch(() => {})
  })

  // the Carbon and Classic themes' fonts come the first time the theme is
  // used, the text shows the fallback fonts until then (offline included)
  $effect(() => {
    if (!downloads) return
    const { ui, editor, mono } = currentTheme().fonts
    for (const family of [ui, editor, mono]) {
      const pack = themeFontPack(family)
      if (pack) void loadFontPack(pack, true).catch(() => {})
    }
  })

  $effect(() => {
    const chosen = chosenTheme()
    remember(isBuiltIn(chosen.id) ? null : chosen)
  })

  // 12 or 24 hours: the choice, and the system's clock read now and again
  // when the window comes back, where it may have changed
  $effect(() => {
    clock.choice = prefs.clock
  })
  $effect(() => {
    const again = () => void readSystemClock(() => backend.systemClock24h())
    again()
    window.addEventListener('focus', again)
    return () => window.removeEventListener('focus', again)
  })

  // the system's accent, read when the setting turns on and again when the
  // window comes back to the front, where it may have changed
  $effect(() => {
    if (!prefs.systemAccent) return
    void readSystemAccent()
    const again = () => void readSystemAccent()
    window.addEventListener('focus', again)
    return () => window.removeEventListener('focus', again)
  })

  $effect(() => {
    const theme = currentTheme()
    const mode = currentMode()
    const root = document.documentElement
    const accent = (prefs.systemAccent && systemAccent.color) || prefs.accent || undefined
    const vars = themeToCssVars(theme, mode, accent, prefs.pureBlack)
    // a chosen font goes first, the theme's stay behind it as fallbacks
    const notes = prefs.notesFont && familyOf(prefs.notesFont)
    const code = prefs.codeFont && familyOf(prefs.codeFont)
    if (notes) vars['--font-editor'] = `${quoted(notes)}, ${vars['--font-editor']}`
    if (code) vars['--font-mono'] = `${quoted(code)}, ${vars['--font-mono']}`
    for (const [name, value] of Object.entries(vars)) root.style.setProperty(name, value)
    root.dataset.mode = mode
    root.dataset.theme = theme.id
    root.style.colorScheme = mode
    themeStyle().textContent = theme.css ?? ''
  })
}

/** The style element holding the theme's css, after the app's own styles. */
function themeStyle(): HTMLStyleElement {
  let style = document.getElementById('theme-css') as HTMLStyleElement | null
  if (!style) {
    style = document.createElement('style')
    style.id = 'theme-css'
    document.head.append(style)
  }
  return style
}
