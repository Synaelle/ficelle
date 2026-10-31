// SPDX-License-Identifier: AGPL-3.0-or-later
// The Android home screen widget cannot open the vault, so the page sends it
// what to show: the app name, up to four recent notes and a message. Titles
// of an encrypted vault never leave the app, the widget gets a message
// instead. See NotesWidget.kt.

import { brand } from '@app/brand'
import { backend } from './backend'
import { t } from './i18n.svelte'

declare global {
  interface Window {
    /** Set by MainActivity on Android. */
    AppWidget?: { update(json: string): void }
  }
}

const RECENT = 4

export async function updateWidget() {
  if (!window.AppWidget) return
  const status = await backend.vaultStatus()
  // a blank note still open in the app is not worth a row, it goes once left
  const recent = status.encrypted
    ? []
    : (await backend.listNotes({ kind: 'all' }, 'updatedDesc'))
        .filter((n) => n.title.trim() || n.snippet.trim())
        .slice(0, RECENT)
  const message = status.encrypted
    ? t('widget.encrypted')
    : recent.length === 0
      ? t('widget.empty')
      : ''
  window.AppWidget.update(
    JSON.stringify({
      title: brand.displayName,
      message,
      notes: recent.map((n) => ({ id: n.id, title: n.title || t('note.untitled') })),
    }),
  )
}
