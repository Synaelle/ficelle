// SPDX-License-Identifier: AGPL-3.0-or-later

import { brand } from '@app/brand'
import { mount } from 'svelte'
import App from './App.svelte'
import { LINUX, MOBILE, PLATFORM } from './lib/platform'
import { followScrollbars } from './lib/scrollbars'
import { isQuickWindow } from './lib/quickNote'
import QuickNote from './QuickNote.svelte'
import './styles/app.css'

document.title = brand.displayName
// for styles that only one system needs (the scrollbars on Linux)
document.documentElement.dataset.platform = PLATFORM
if (LINUX) followScrollbars()

const target = document.getElementById('app')
if (!target) throw new Error('missing #app element in index.html')

// the desktop quick note window loads the same page with #quick
// (desktop only: the phone builds leave the quick note out)
export default mount(!MOBILE && isQuickWindow() ? QuickNote : App, { target })
