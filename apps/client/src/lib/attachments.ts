// SPDX-License-Identifier: AGPL-3.0-or-later
// How notes point at their attachments: a relative link the editor resolves
// to the stored file, so exported markdown keeps working next to its files.

import type { Attachment } from './backend'

export const ATTACHMENT_PREFIX = 'attachments/'

/** Markdown the editor writes for an attachment: an image or a link. */
export function attachmentMarkdown(attachment: Attachment, mime: string): string {
  const src = ATTACHMENT_PREFIX + encodeURI(attachment.fileName)
  return mime.startsWith('image/')
    ? `![${attachment.fileName.replace(/\.[^.]+$/, '')}](${src})`
    : `[${attachment.fileName}](${src})`
}
