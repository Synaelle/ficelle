// SPDX-License-Identifier: AGPL-3.0-or-later
// The admin page, served by the server at /admin. It shares the app's
// themes, translations and controls, and talks only to its own server.

import { brand } from '@app/brand'
import { mount } from 'svelte'
import '../styles/app.css'
// the apps download these when a theme needs them, the page carries them
import '../styles/fonts-themes.css'
import Admin from './Admin.svelte'

document.title = `${brand.displayName} admin`

const target = document.getElementById('app')
if (!target) throw new Error('missing #app element in admin.html')

export default mount(Admin, { target })
