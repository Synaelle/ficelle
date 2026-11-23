// SPDX-License-Identifier: AGPL-3.0-or-later
// The menu of one tag, the same in the sidebar and in Manage tags.

import { TAG_COLORS, type Tag } from './backend'
import { t } from './i18n.svelte'
import { overlay, SEPARATOR, type MenuItem } from './overlay.svelte'
import { tagLabel, tagLeaf } from './tagMark.svelte'
import { tagWithin, workspace } from './workspace.svelte'

/** `rename` opens the place's own name field, `attempt` runs a change and
 * says a failure where the menu was opened, `unfold` shows the tags
 * inside once one is made there. */
export function tagMenuItems(
  tag: Tag,
  {
    rename,
    attempt,
    unfold,
  }: {
    rename: () => void
    attempt: (change: () => Promise<unknown>) => Promise<void>
    unfold?: () => void
  },
): MenuItem[] {
  const others = workspace.tags.filter((o) => !tagWithin(o.name, tag.name))

  async function newInside() {
    const name = await overlay.askText(t('tags.newInside', { name: tagLeaf(tag.name) }))
    if (!name?.trim()) return
    unfold?.()
    await attempt(() => workspace.createTag(`${tag.name}/${name}`))
  }

  async function remove() {
    const yes = await overlay.ask(
      t('tags.confirmDelete', { name: tagLabel(tag.name), count: tag.noteCount }),
      { confirmLabel: t('tags.delete'), danger: true },
    )
    if (yes) await attempt(() => workspace.deleteTag(tag.id))
  }

  return [
    { label: t('tags.newInsideMenu'), icon: 'plus', run: () => void newInside() },
    { label: t('tags.rename'), icon: 'pencil', run: rename },
    {
      label: t('tags.merge'),
      icon: 'move',
      disabled: others.length === 0,
      items: others.map((other) => ({
        label: tagLabel(other.name),
        run: () => void attempt(() => workspace.renameTag(tag.id, other.name)),
      })),
    },
    {
      label: t('tags.colour'),
      icon: 'palette',
      items: [
        {
          label: t('tags.noColour'),
          checked: !tag.color,
          run: () => void attempt(() => workspace.setTagColor(tag.id, null)),
        },
        ...TAG_COLORS.map((color) => ({
          label: t(`tags.colour_${color}`),
          checked: tag.color === color,
          run: () => void attempt(() => workspace.setTagColor(tag.id, color)),
        })),
      ],
    },
    SEPARATOR,
    {
      label: t('tags.deleteFrom', { count: tag.noteCount }),
      icon: 'trash',
      danger: true,
      run: () => void remove(),
    },
  ]
}
