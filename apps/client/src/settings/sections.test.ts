// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { setLocale, t } from '../lib/i18n.svelte'
import { prefs } from '../lib/prefs.svelte'
import { sectionLabel, settingsInside, settingsSections, type ChoiceItem } from './sections.svelte'
import { SettingsState } from './settingsState.svelte'

const sections = settingsSections(new SettingsState(), () => {})
const items = sections.flatMap((s) => s.groups.flatMap((g) => g.items))

describe('settingsSections', () => {
  it('names every section and setting in English', async () => {
    await setLocale('en')
    const keys = [...sections.map((s) => sectionLabel(s.id)), ...items.map((i) => i.label)]
    expect(keys.filter((key) => t(key) === key)).toEqual([])
  })

  it('lists each setting once', () => {
    const labels = items.map((i) => i.label)
    expect(labels.filter((label, i) => labels.indexOf(label) !== i)).toEqual([])
  })

  it('saves a choice in the prefs', () => {
    const theme = items.find((i): i is ChoiceItem => i.label === 'settings.theme')
    theme
      ?.options()
      .find((o) => o.label === t('settings.theme_light'))
      ?.pick()
    expect(prefs.mode).toBe('light')
    expect(theme?.options().find((o) => o.picked)?.label).toBe(t('settings.theme_light'))
  })
})

describe('settingsInside', () => {
  it('finds the settings by their section, the security rows on their own', () => {
    const inside = settingsInside(sections)
    expect(inside).toContainEqual(['code', 'settings.codeFont', 'editor'])
    expect(inside).toContainEqual(['backup', 'backups.title', 'data'])
    expect(inside.some(([, label]) => label === 'settings.autoLock')).toBe(false)
  })
})
