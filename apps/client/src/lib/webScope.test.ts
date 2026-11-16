// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { scopeOf, scopedStorage } from './webScope'

describe('scopeOf', () => {
  it('gives each path its own scope', () => {
    expect(scopeOf('/web/')).toBe('%2Fweb')
    expect(scopeOf('/app/')).toBe('%2Fapp')
    expect(scopeOf('/')).toBe('%2F')
    expect(scopeOf('/notes/app/')).toBe('%2Fnotes%2Fapp')
  })

  it('never gives two paths the same scope', () => {
    const paths = ['/', '/web/', '/app/', '/a-b/', '/a/b/', '/A/', '/a/', '/root/', '/web/app/']
    const scopes = paths.map(scopeOf)
    expect(new Set(scopes).size).toBe(paths.length)
  })
})

describe('scopedStorage', () => {
  it('keeps two scopes apart, clear() included', () => {
    const real = window.localStorage
    real.clear()
    real.setItem('prefs', 'from before scopes')
    const web = scopedStorage(real, '%2Fweb')
    const app = scopedStorage(real, '%2Fapp')
    app.setItem('prefs', 'mine')
    web.setItem('prefs', 'public')
    expect(app.getItem('prefs')).toBe('mine')
    expect(web.getItem('prefs')).toBe('public')
    expect(web.length).toBe(1)
    expect(web.key(0)).toBe('prefs')
    // what was kept before scopes is no scope's
    expect(web.getItem('prefs')).not.toBe('from before scopes')
    web.clear()
    expect(web.getItem('prefs')).toBeNull()
    expect(app.getItem('prefs')).toBe('mine')
    expect(real.getItem('prefs')).toBe('from before scopes')
    app.removeItem('prefs')
    expect(app.length).toBe(0)
    real.clear()
  })
})
