// SPDX-License-Identifier: AGPL-3.0-or-later
// Product names and identifiers for the ui. The values live in brand.json,
// next to this file, so a rename touches a single file. Previews take the preview
// identity and other non release builds the dev one, same rule as crates/brand.

import values from './brand.json' with { type: 'json' }

/** Injected by the client vite config, see tools/version.mjs. */
declare const __APP_RELEASE__: boolean
declare const __APP_PREVIEW__: boolean

const { dev, preview, ...release } = values

export const brand = __APP_PREVIEW__
  ? { ...release, ...preview }
  : __APP_RELEASE__
    ? release
    : { ...release, ...dev }
