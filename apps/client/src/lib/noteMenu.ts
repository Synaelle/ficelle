// SPDX-License-Identifier: AGPL-3.0-or-later
// Menu items shared by the note list and the open note.

import type { Id } from './backend'
import { t } from './i18n.svelte'
import type { MenuItem } from './overlay.svelte'
import { workspace } from './workspace.svelte'

/** Folders to move a note to, the current one checked. */
export function moveToItems(id: Id, current: Id | null): MenuItem[] {
  return [
    {
      label: t('note.noFolder'),
      checked: current === null,
      run: () => void workspace.move(id, null),
    },
    ...workspace.folders.map((folder) => ({
      label: folder.name,
      icon: 'folder' as const,
      checked: current === folder.id,
      run: () => void workspace.move(id, folder.id),
    })),
  ]
}
