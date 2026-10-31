// SPDX-License-Identifier: AGPL-3.0-or-later
// What the instance this device is signed in to says about themes: the
// ones it offers, and whether others may be used. The last rules heard are
// remembered, so they hold while the device is offline, and are forgotten
// when the device signs out.

import { backend } from './backend'
import type { CatalogState, Extensions } from './backend/types'
import { prefs } from './prefs.svelte'
import { importTheme, setThemeFilter } from './theme.svelte'

const KEY = 'instance-extensions'
/** How often the rules are asked for again, at most. */
const EVERY_MS = 5 * 60 * 1000

/** Whether the rules let an imported theme be used: under "only the
 * instance's", one it offers (or its catalog does) and added from it. */
export function themeAllowed(
  id: string,
  extensions: Extensions | null,
  catalogIds: readonly string[] = [],
): boolean {
  if (!extensions || extensions.rules.themes === 'any') return true
  const offered = extensions.themes.some((t) => t.id === id) || catalogIds.includes(id)
  return offered && id in prefs.instanceThemes
}

function remembered(): Extensions | null {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? 'null') as Extensions | null
  } catch {
    return null
  }
}

class Instance {
  extensions: Extensions | null = $state(remembered())
  /** The official catalog as this device sees it, null until asked. */
  catalog: CatalogState | null = $state(null)

  constructor() {
    setThemeFilter((id) => themeAllowed(id, this.extensions, this.serverCatalog()))
  }

  /** Ids of the catalog's themes when it comes through the server, which
   * then offers them as its own. */
  serverCatalog(): string[] {
    const c = this.catalog
    if (!c || c.via !== 'server' || !c.enabled) return []
    return (c.catalog?.entries ?? []).filter((e) => e.kind === 'theme').map((e) => e.id)
  }

  /** Reads the catalog again: through the server when signed in, straight
   * from it when the person turned it on, else none. */
  async loadCatalog() {
    if (!backend.native) return
    try {
      this.catalog = await backend.catalogList(prefs.catalogLocal)
    } catch (err) {
      const message = (err as { message?: string }).message ?? String(err)
      this.catalog = {
        available: true,
        via: 'none',
        enabled: false,
        catalog: null,
        error: message,
        host: this.catalog?.host ?? '',
      }
    }
  }

  /** Downloads a catalog theme, checked, and keeps it. */
  async addCatalogTheme(id: string) {
    const entry = this.catalog?.catalog?.entries.find((e) => e.kind === 'theme' && e.id === id)
    if (!entry) return
    const text = await backend.catalogTheme(id, prefs.catalogLocal)
    const result = await importTheme(text)
    if (!result.ok) throw new Error(result.problems.join(', '))
    // the signed file, whichever way it came: one of the server's themes
    // when the server serves the catalog
    prefs.instanceThemes = { ...prefs.instanceThemes, [id]: entry.contentSha256 }
  }
  private askedAt = 0

  /** Asks the server again, unless it was asked a moment ago. */
  async refresh(force = false) {
    if (!backend.native) return
    if (!force && Date.now() - this.askedAt < EVERY_MS) return
    this.askedAt = Date.now()
    let next: Extensions | null
    try {
      next = await backend.instanceExtensions()
    } catch {
      // offline or refused: the last rules heard stay
      return
    }
    this.extensions = next
    try {
      if (next) localStorage.setItem(KEY, JSON.stringify(next))
      else localStorage.removeItem(KEY)
    } catch {
      // storage can be unavailable, the rules hold until the app closes
    }
    await this.addThemes()
    await this.loadCatalog()
  }

  /** Themes the instance adds on every device: no permission to ask for. */
  private async addThemes() {
    const offered = this.extensions?.themes.filter((t) => t.preinstall) ?? []
    for (const theme of offered) {
      if (prefs.instanceThemes[theme.id] === theme.sha256) continue
      await this.addTheme(theme.id).catch(() => {})
    }
  }

  /** Downloads a theme the instance offers and keeps it. */
  async addTheme(id: string) {
    const offered = this.extensions?.themes.find((t) => t.id === id)
    if (!offered) return
    const text = await backend.instanceTheme(id)
    const result = await importTheme(text)
    if (result.ok) prefs.instanceThemes = { ...prefs.instanceThemes, [id]: offered.sha256 }
  }
}

export const instance = new Instance()

// dev builds let the end to end tests ask again at once
if (__APP_DEV_REPORTS__) (window as { __instance?: Instance }).__instance = instance
