// SPDX-License-Identifier: AGPL-3.0-or-later
// Built-in themes. Only the default one for now, others will be offered
// for download instead.

import type { Theme } from '../src/theme'
import darkroom from './darkroom.json' with { type: 'json' }

export const builtInThemes = [darkroom] as Theme[]

export const DEFAULT_THEME_ID = 'darkroom'
