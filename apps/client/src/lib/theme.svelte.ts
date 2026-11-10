// SPDX-License-Identifier: AGPL-3.0-or-later
// Applies the app's one theme to the page as css variables, dark unless the
// person chose light in Appearance.

import { themeToCssVars, type Mode, type Theme } from '@app/ui/theme'
import { builtInThemes, DEFAULT_THEME_ID } from '@app/ui/themes'
import { ARROWS_FONT } from './fonts'
import { prefs } from './prefs.svelte'
import { clock, readSystemClock } from './clock.svelte'

export function currentTheme(): Theme {
  return builtInThemes.find((t) => t.id === DEFAULT_THEME_ID) ?? (builtInThemes[0] as Theme)
}

export const MODES: Mode[] = ['dark', 'light']

export function currentMode(): Mode {
  return prefs.mode
}

/** Keeps the page styled. Call once from the root component, with how to
 * read the system's 12 or 24 hour clock where the page can. */
export function applyTheme(systemClock24h?: () => Promise<boolean | null>) {
  // 12 or 24 hours: the choice, and the system's clock read now and again
  // when the window comes back, where it may have changed
  $effect(() => {
    clock.choice = prefs.clock
  })
  $effect(() => {
    if (!systemClock24h) return
    const again = () => void readSystemClock(systemClock24h)
    again()
    window.addEventListener('focus', again)
    return () => window.removeEventListener('focus', again)
  })

  $effect(() => {
    const theme = currentTheme()
    const mode = currentMode()
    const root = document.documentElement
    const vars = themeToCssVars(theme, mode, prefs.pureBlack)
    // the app's fonts have no arrows: ours come right after the first font,
    // so an arrow it lacks looks the same everywhere (styles/arrows.css)
    for (const name of ['--font-editor', '--font-ui']) {
      const stack = vars[name]
      if (stack) vars[name] = stack.replace(/^([^,]+)/, `$1, ${ARROWS_FONT}`)
    }
    for (const [name, value] of Object.entries(vars)) root.style.setProperty(name, value)
    root.dataset.mode = mode
    // for what stays a shade of grey otherwise: phone settings go outlined
    root.dataset.black = String(mode === 'dark' && prefs.pureBlack)
    window.AppSystem?.setBars?.(vars['--color-bg'] ?? '', mode === 'light')
    root.style.colorScheme = mode
  })
}
