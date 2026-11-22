// SPDX-License-Identifier: AGPL-3.0-or-later
// Menu items shared by the note list and the open note.

import type { Id } from './backend'
import type { MenuItem } from './overlay.svelte'
import { tagLabel, tagLeaf } from './tagMark.svelte'
import { tagKey, workspace } from './workspace.svelte'

/** Every tag, the note's own checked: picking one adds or takes it off. */
export function tagItems(id: Id, own: string[]): MenuItem[] {
  const has = new Set(own.map(tagKey))
  // a tree as in the sidebar, each tag by its last level: two Gardens
  // under different parents still read apart
  return workspace.tags.map((tag) => ({
    label: tagLabel(tagLeaf(tag.name)),
    depth: tag.name.split('/').length - 1,
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
