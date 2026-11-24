// SPDX-License-Identifier: AGPL-3.0-or-later
// Where the OpenDyslexic setting's download stands, for the settings pages
// (lib/dyslexicFont.ts sets it).

import type { ShownError } from './errors'

export const dyslexicStatus: { loading: boolean; error: ShownError | null } = $state({
  loading: false,
  error: null,
})
