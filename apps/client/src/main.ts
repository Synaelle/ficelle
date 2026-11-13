// SPDX-License-Identifier: AGPL-3.0-or-later

import { brand } from '@app/brand'
import { mount } from 'svelte'
import App from './App.svelte'
import { setLocale, systemLocale } from './lib/i18n.svelte'
import { DESKTOP, HANDHELD, LINUX, PLATFORM } from './lib/platform'
import { prefs } from './lib/prefs.svelte'
import { followScrollbars } from './lib/scrollbars'
import { isQuickWindow } from './lib/quickNote'
import QuickNote from './QuickNote.svelte'
import './styles/app.css'

document.title = brand.displayName
// for styles that only one system needs (the scrollbars on Linux)
document.documentElement.dataset.platform = PLATFORM
if (LINUX) followScrollbars()
// the web version on a phone: a keyboard that covers the page instead of
// shrinking it (Safari) leaves the page the height still in sight. Times the
// zoom, so pinching in does not shrink the page with it
const viewport = window.visualViewport
if (PLATFORM === 'web' && HANDHELD && viewport) {
  const follow = () =>
    document.documentElement.style.setProperty(
      '--visible-height',
      `${viewport.height * viewport.scale}px`,
    )
  viewport.addEventListener('resize', follow)
  follow()
}

// the language's texts are here before the first screen, never English first
await setLocale(prefs.language ?? systemLocale())

const target = document.getElementById('app')
if (!target) throw new Error('missing #app element in index.html')

// the desktop quick note window loads the same page with #quick
// (desktop apps only: the phone and the web leave the quick note out)
export default mount(DESKTOP && isQuickWindow() ? QuickNote : App, { target })
