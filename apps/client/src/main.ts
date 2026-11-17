// SPDX-License-Identifier: AGPL-3.0-or-later

// first: the web version's storage is its own path's before anything reads it
// (the web build keeps modules in this order, see vite.config.ts)
import './lib/webStorage'
import { brand } from '@app/brand'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { mount } from 'svelte'
import App from './App.svelte'
import { setLocale, systemLocale } from './lib/i18n.svelte'
import { DESKTOP, HANDHELD, LINUX, PLATFORM, WEB } from './lib/platform'
import { prefs } from './lib/prefs.svelte'
import { followScrollbars } from './lib/scrollbars'
import { followVisibleHeight } from './lib/uiSize'
import { isQuickWindow } from './lib/quickNote'
import QuickNote from './QuickNote.svelte'
import FileWindow from './FileWindow.svelte'
import NoteWindow from './NoteWindow.svelte'
import { windowKind } from './lib/windows'
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

// the desktop's other windows load the same page: #quick for the quick note,
// #file for a file opened with the app, #note=<id> for a note of its own
// (desktop apps only: the phone and the web have one window)
const kind = DESKTOP ? windowKind() : { kind: 'main' as const }
export default DESKTOP && isQuickWindow()
  ? mount(QuickNote, { target })
  : kind.kind === 'file'
    ? mount(FileWindow, { target, props: { close: () => void getCurrentWindow().destroy() } })
    : kind.kind === 'note'
      ? mount(NoteWindow, { target, props: { id: kind.id } })
      : mount(App, { target })
