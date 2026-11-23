// SPDX-License-Identifier: AGPL-3.0-or-later
// The formatting buttons by the icon each shows, in the toolbar's order, for
// the setting that hides some of them.
import type { IconName } from '../lib/icons/sets'

export const TOOL_NAMES: readonly { icon: IconName; label: string }[] = [
  { icon: 'bold', label: 'editor.bold' },
  { icon: 'italic', label: 'editor.italic' },
  { icon: 'strike', label: 'editor.strike' },
  { icon: 'heading', label: 'editor.heading' },
  { icon: 'list', label: 'editor.bulletList' },
  { icon: 'listOrdered', label: 'editor.orderedList' },
  { icon: 'tasks', label: 'editor.checklist' },
  { icon: 'quote', label: 'editor.quote' },
  { icon: 'inlineCode', label: 'editor.inlineCode' },
  { icon: 'code', label: 'editor.code' },
  { icon: 'link', label: 'editor.link' },
  { icon: 'table', label: 'editor.table' },
  { icon: 'image', label: 'editor.image' },
]
