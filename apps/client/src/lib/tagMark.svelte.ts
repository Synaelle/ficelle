// SPDX-License-Identifier: AGPL-3.0-or-later
// How a tag's path reads, by the mark chosen in Settings, Appearance: with
// the folder mark "projects › garden", with # "#projects/garden".

import { prefs } from './prefs.svelte'

/** The marks to choose from. */
export const TAG_MARKS = ['folder', 'hash'] as const

/** A tag's whole path, as rows and notes show it. */
export function tagLabel(name: string): string {
  return prefs.tagMark === 'hash' ? `#${name}` : name.split('/').join(' › ')
}

/** The last level of a tag, as the sidebar tree shows it. */
export function tagLeaf(name: string): string {
  return name.slice(name.lastIndexOf('/') + 1)
}

/** What a note's chip shows for `name` among its `tags`: the last level,
 * or as many levels as tell it apart from another tag ending the same way
 * (Paperwork/insurance and admin/insurance, not insurance twice). */
export function tagShort(name: string, tags: readonly string[]): string {
  const parts = name.split('/')
  for (let n = 1; n < parts.length; n++) {
    const end = parts.slice(-n).join('/')
    const same = tags.some(
      (other) => other !== name && (other === end || other.endsWith(`/${end}`)),
    )
    if (!same) return end
  }
  return name
}

/** The levels above a tag, empty at the top. */
export function tagParent(name: string): string {
  const at = name.lastIndexOf('/')
  return at < 0 ? '' : name.slice(0, at)
}
