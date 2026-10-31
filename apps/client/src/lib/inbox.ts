// SPDX-License-Identifier: AGPL-3.0-or-later
// What the system hands over while the app is busy or locked: shares from
// other apps ("share to"), the icon's quick actions and widget taps. Android
// keeps them in MainActivity's Inbox.kt until the workspace takes them here,
// once the notes are open. Other platforms have no inbox yet and take nothing.

import { attachmentMarkdown } from './attachments'
import { backend } from './backend'
import { workspace } from './workspace.svelte'

interface SharedFile {
  path: string
  name: string
  mime: string
}

export type InboxItem =
  | { kind: 'share'; text: string; subject: string; files: SharedFile[] }
  | { kind: 'newNote' }
  | { kind: 'search' }
  | { kind: 'openNote'; id: string }

declare global {
  interface Window {
    /** Set by MainActivity on Android. */
    AppInbox?: { take(): string }
    /** Called by MainActivity when something arrives. */
    __appInbox?: () => void
  }
}

export function takeInbox(): InboxItem[] {
  try {
    const items: unknown = JSON.parse(window.AppInbox?.take() ?? '[]')
    return Array.isArray(items) ? (items as InboxItem[]) : []
  } catch {
    return []
  }
}

/**
 * A new note from a share: the subject (a page title, an email subject) as
 * title, the shared text as body, files attached below it. It opens once
 * complete, the editor reads a body only when its note opens.
 */
export async function saveShare(item: Extract<InboxItem, { kind: 'share' }>) {
  const note = await workspace.create({ title: item.subject.trim(), body: item.text.trim() }, false)
  const lines: string[] = []
  for (const file of item.files) {
    try {
      const attachment = await backend.attachInboxFile(note.id, file.path, file.name, file.mime)
      lines.push(attachmentMarkdown(attachment, file.mime))
    } catch (err) {
      workspace.report(err)
    }
  }
  if (lines.length > 0) {
    const body = [note.body, ...lines].filter(Boolean).join('\n\n')
    await backend.updateNote(note.id, { body })
  }
  await workspace.open(note.id)
}
