// SPDX-License-Identifier: AGPL-3.0-or-later
// Errors as the admin page shows them, dates and sizes from lib/format.

import { t } from '../lib/i18n.svelte'
import { ApiError } from './api.svelte'

export { bytes, date, dateTime, relative } from '../lib/format'

/** What to tell the admin about a failed call. */
export function errorText(err: unknown): string {
  if (err instanceof ApiError) {
    const key = `admin.errors.${err.code}`
    const text = t(key, { seconds: err.retryAfter ?? 0 })
    return text === key ? err.message : text
  }
  return t('admin.errors.internal')
}
