// SPDX-License-Identifier: AGPL-3.0-or-later

// first: the web version's storage is its own path's before anything reads it
// (the web build keeps modules in this order, see vite.config.ts)
import './lib/webStorage'
import { brand } from '@app/brand'
import { mount } from 'svelte'
import App from './App.svelte'
import { setLocale, systemLocale } from './lib/i18n.svelte'
import { DESKTOP, HANDHELD, LINUX, PLATFORM, WEB } from './lib/platform'
import { prefs } from './lib/prefs.svelte'
import { followScrollbars } from './lib/scrollbars'
import { followVisibleHeight } from './lib/uiSize'
import { isQuickWindow } from './lib/quickNote'
import QuickNote from './QuickNote.svelte'
import { isScoped } from './lib/webScope'
import './styles/app.css'

// webStorage.ts ran first or the web version opens nothing: another path's
// settings, or its notes, must never be this page's
if (WEB && !(isScoped(localStorage) && isScoped(sessionStorage)))
  throw new Error('the web version started without its own storage')

document.title = brand.displayName
// for styles that only one system needs (the scrollbars on Linux)
document.documentElement.dataset.platform = PLATFORM
if (LINUX) followScrollbars()
if (PLATFORM === 'web' && HANDHELD) followVisibleHeight()

// the language's texts are here before the first screen, never English first
await setLocale(prefs.language ?? systemLocale())

const target = document.getElementById('app')
if (!target) throw new Error('missing #app element in index.html')

// the desktop quick note window loads the same page with #quick
// (desktop apps only: the phone and the web leave the quick note out)
export default mount(DESKTOP && isQuickWindow() ? QuickNote : App, { target })
