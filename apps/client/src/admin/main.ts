// SPDX-License-Identifier: AGPL-3.0-or-later
// The admin page, served by the server at /admin. It shares the app's
// themes, texts and controls, and talks only to its own server. It is in
// English only, its own texts kept apart so the apps never carry them.

import { brand } from '@app/brand'
import { mount } from 'svelte'
import texts from '../../../../packages/locales/server/texts.toml'
import { addEnglish } from '../lib/i18n.svelte'
import '../styles/app.css'
import './admin.css'
import Admin from './Admin.svelte'

addEnglish(texts)
document.title = `${brand.displayName} admin`

const target = document.getElementById('app')
if (!target) throw new Error('missing #app element in admin.html')

export default mount(Admin, { target })
