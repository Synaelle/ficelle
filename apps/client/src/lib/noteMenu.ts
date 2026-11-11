// SPDX-License-Identifier: AGPL-3.0-or-later
// Menu items shared by the note list and the open note.

import type { Id } from './backend'
import type { MenuItem } from './overlay.svelte'
import { tagLabel } from './tagMark.svelte'
import { tagKey, workspace } from './workspace.svelte'

/** Every tag, the note's own checked: picking one adds or takes it off. */
export function tagItems(id: Id, own: string[]): MenuItem[] {
  const has = new Set(own.map(tagKey))
  return workspace.tags.map((tag) => ({
    label: tagLabel(tag.name),
    checked: has.has(tagKey(tag.name)),
    run: () =>
      void workspace.setNoteTags(
        id,
        has.has(tagKey(tag.name))
          ? own.filter((t) => tagKey(t) !== tagKey(tag.name))
          : [...own, tag.name],
      ),
  }))
}
