// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { appSystem, downloadPage } from './appSystem'

const seen = (userAgent: string, platform?: string) => appSystem({ userAgent, platform })

describe('appSystem', () => {
  it('tells the systems with an app', () => {
    expect(seen('Mozilla/5.0 (Windows NT 10.0; Win64; x64) Firefox/140.0')).toBe('windows')
    expect(seen('Mozilla/5.0 (X11; Linux x86_64) Firefox/140.0')).toBe('linux')
    expect(seen('Mozilla/5.0 (Android 15; Mobile; rv:140.0) Firefox/140.0')).toBe('android')
    expect(seen('Mozilla/5.0 (Linux; Android 15; Pixel 7) Chrome/140.0 Mobile')).toBe('android')
  })

  it('offers nothing where there is no app', () => {
    expect(seen('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Safari/605.1.15')).toBeNull()
    expect(seen('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) Safari/604.1')).toBeNull()
    expect(seen('Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) Chrome/140.0')).toBeNull()
    expect(seen('Mozilla/5.0 (X11; FreeBSD amd64; rv:140.0) Firefox/140.0')).toBeNull()
    expect(seen('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Haiku')).toBeNull()
    expect(seen('curl/8.0')).toBeNull()
  })

  it('takes the client hints when the browser gives them', () => {
    expect(seen('Mozilla/5.0 (X11; Linux x86_64) Chrome/140.0', 'Chrome OS')).toBeNull()
    expect(seen('Mozilla/5.0 Chrome/140.0', 'Windows')).toBe('windows')
  })
})

describe('downloadPage', () => {
  it('opens the website on the system', () => {
    expect(downloadPage('android')).toBe('https://ficelle.synaelle.com/?os=android')
  })
})
