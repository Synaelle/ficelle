// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest'
import { ACTIONS, bindingOf, DEFAULT_SHORTCUTS, displayBinding, isMac } from './shortcuts'

const key = (init: KeyboardEventInit) => new KeyboardEvent('keydown', init)

describe('shortcuts', () => {
  it('names a key event like the settings do', () => {
    const mod = isMac ? { metaKey: true } : { ctrlKey: true }
    expect(bindingOf(key({ key: 'n', ...mod }))).toBe('Mod+N')
    expect(bindingOf(key({ key: 'P', shiftKey: true, ...mod }))).toBe('Mod+Shift+P')
    expect(bindingOf(key({ key: 'Shift', shiftKey: true }))).toBeNull()
  })

  it('has no two actions on the same default binding', () => {
    const bindings = ACTIONS.map((a) => DEFAULT_SHORTCUTS[a])
    expect(new Set(bindings).size).toBe(bindings.length)
  })

  it('shows bindings for the platform', () => {
    expect(displayBinding('Mod+N')).toBe(isMac ? '⌘N' : 'Ctrl+N')
  })
})
