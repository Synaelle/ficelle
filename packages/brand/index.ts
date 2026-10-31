// SPDX-License-Identifier: AGPL-3.0-or-later
// Product names and identifiers for the ui. The values live in brand.json,
// next to this file, so a rename touches a single file. Non release builds take the
// dev identity, same rule as crates/brand.

import values from './brand.json' with { type: 'json' }

/** Injected by the client vite config, see tools/version.mjs. */
declare const __APP_RELEASE__: boolean

const { dev, ...release } = values

export const brand = __APP_RELEASE__ ? release : { ...release, ...dev }
