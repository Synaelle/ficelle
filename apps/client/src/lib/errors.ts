// SPDX-License-Identifier: AGPL-3.0-or-later
// Errors as the pages show them: why, in words, and a short code the person
// can send back. The code says where it happened and what went wrong, in
// capitals, SAVE-DATABASE for an edit the database refused. A failure with
// no reason of its own says its message instead, on one line.

import type { BackendError } from './backend/types'
import { t } from './i18n.svelte'

export interface ShownError {
  /** The backend's kind of error, `unknown` for a crash. */
  kind: string
  /** Why, in words. */
  reason: string
  /** AREA-KIND, see errorCode. */
  code: string
}

/** Kinds that name no reason: their message is the reason. */
const UNNAMED = ['unknown', 'internal']
const DETAIL_MAX = 160

export function isBackendError(err: unknown): err is BackendError {
  return typeof err === 'object' && err !== null && 'kind' in err
}

export const errorKind = (err: unknown): string => (isBackendError(err) ? err.kind : 'unknown')

/** The code for `kind` in `area`: SAVE and wrongKey make SAVE-WRONG-KEY. */
export function errorCode(area: string, kind: string): string {
  const words = kind.replace(/([a-z0-9])([A-Z])/g, '$1-$2').replace(/[_\s]+/g, '-')
  return `${area}-${words}`.toUpperCase()
}

/** The error's own message on one short line, empty when it has none. */
export function errorDetail(err: unknown): string {
  const message =
    typeof err === 'string'
      ? err
      : typeof err === 'object' && err !== null && 'message' in err
        ? err.message
        : ''
  const line = typeof message === 'string' ? message.replace(/\s+/g, ' ').trim() : ''
  return line.length > DETAIL_MAX ? `${line.slice(0, DETAIL_MAX - 1)}…` : line
}

/** Why a `kind` of error happened, with its message where the text has a
 * place for it (errors.<kind>Detail). A kind without a text of its own is
 * an unexpected error, said with its message. */
export function errorReason(kind: string, detail = ''): string {
  const named = !UNNAMED.includes(kind) && t(`errors.${kind}`) !== `errors.${kind}`
  const base = named ? kind : 'unknown'
  if (detail) {
    const key = `errors.${base}Detail`
    const text = t(key, { detail })
    if (text !== key) return text
  }
  return t(`errors.${base}`)
}

/** A caught error as a page shows it, for a failure in `area`. */
export function shownError(err: unknown, area: string): ShownError {
  const kind = errorKind(err)
  return { kind, reason: errorReason(kind, errorDetail(err)), code: errorCode(area, kind) }
}

/** An error the app finds itself, a `kind` with a reason in errors. */
export const namedError = (kind: string, area: string): ShownError => ({
  kind,
  reason: errorReason(kind),
  code: errorCode(area, kind),
})

/** Reason and code as one text, for places that only show text. */
export const errorLine = (error: ShownError): string =>
  `${error.reason} ${t('common.errorCode', { code: error.code })}`
