// SPDX-License-Identifier: AGPL-3.0-or-later
// The system a visitor of the web version is on, when there is an app for
// it, told from the browser the way the website's download page tells it.
import { brand } from '@app/brand'

export type AppSystem = 'windows' | 'linux' | 'android'

export const SYSTEM_NAMES: Record<AppSystem, string> = {
  windows: 'Windows',
  linux: 'Linux',
  android: 'Android',
}

interface Browser {
  userAgent: string
  /** navigator.userAgentData.platform where the browser has it */
  platform?: string
}

/** The system with an app, or null on one without (the Mac, iOS, ChromeOS,
 * the BSDs, Haiku) or one that cannot be told. */
export function appSystem({ userAgent: ua, platform = '' }: Browser): AppSystem | null {
  const hint = platform.toLowerCase()
  // Android says Linux too, so it comes first
  if (hint === 'android' || /Android/i.test(ua)) return 'android'
  if (/iPhone|iPad|iPod/.test(ua)) return null
  if (hint === 'chrome os' || /CrOS/.test(ua)) return null
  // Haiku's browser calls itself a Mac, the BSDs say X11 like Linux
  if (/Haiku|FreeBSD|OpenBSD|NetBSD|DragonFly/.test(ua)) return null
  if (hint === 'windows' || /Windows/.test(ua)) return 'windows'
  if (hint === 'macos' || /Macintosh|Mac OS X/.test(ua)) return null
  if (hint === 'linux' || /Linux|X11/.test(ua)) return 'linux'
  return null
}

/** This browser's system, when it has an app. */
export function thisAppSystem(): AppSystem | null {
  const seen = navigator as Navigator & { userAgentData?: { platform?: string } }
  return appSystem({ userAgent: navigator.userAgent, platform: seen.userAgentData?.platform })
}

/** The website's download page, opened on `system`'s files. */
export function downloadPage(system: AppSystem): string {
  const page = new URL('/', brand.releasesUrl)
  page.searchParams.set('os', system)
  return page.href
}
