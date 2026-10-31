// SPDX-License-Identifier: AGPL-3.0-or-later
// The editor schema. Used by the editor component and by the round trip
// tests, so both always agree on what markdown the app understands.

import { Extension, type AnyExtension } from '@tiptap/core'
import { Placeholder } from '@tiptap/extensions'
import { BulletList, OrderedList, TaskItem, TaskList } from '@tiptap/extension-list'
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table'
import StarterKit from '@tiptap/starter-kit'
import { Marked, type marked } from 'marked'
import { Callout } from './callout'
import { Details, DetailsSummary } from './details'
import { EmojiShortcode, EmojiTyping } from './emoji'
import { FootnoteDef, FootnoteRef } from './footnote'
import { LanguageCodeBlock } from './codeBlock'
import { LayoutImage } from './image'
import {
  ReadableMarkdown,
  SafeParagraph,
  TASK_GUARD,
  taskListExtent,
  withBoundedStart,
} from './markdownFixes'
import { MathBlock, MathInline } from './math'
import { WikiLink, type WikiLinkOptions } from './wikiLink'

export interface EditorSetup {
  resolveSrc?: (src: string) => string
  wiki?: Partial<WikiLinkOptions>
  /** User key bindings in TipTap form ("Mod-b") for formatting commands. */
  keys?: Record<string, () => boolean>
  /** Turn "- " and "1. " at a line start into lists while typing. Default on. */
  autoLists?: boolean
  /** Turn GitHub's `:name:` shortcodes into their emoji, typed or read.
   * Default on; off, they stay as written. */
  emojiShortcodes?: boolean
  /** Shown in an empty note, so a new one says where to write. Read each
   * time it shows, so it follows the language. */
  placeholder?: () => string
  /** The code block language picker's words, English by default. */
  codeLabels?: { picker: () => string; plain: () => string }
}

/** The list node without its typing shortcut, the toolbar still makes lists. */
function withoutTypingRule<T extends typeof BulletList | typeof OrderedList>(list: T): T {
  return list.extend({ addInputRules: () => [] }) as T
}

export function createExtensions(setup: EditorSetup = {}): AnyExtension[] {
  return [
    StarterKit.configure({
      paragraph: false,
      codeBlock: false,
      bulletList: false,
      orderedList: false,
      // links are typed as markdown or pasted, auto linking would surprise
      link: { openOnClick: false, autolink: false },
    }),
    SafeParagraph,
    LanguageCodeBlock.configure(
      setup.codeLabels
        ? { pickerLabel: setup.codeLabels.picker, plainLabel: setup.codeLabels.plain }
        : {},
    ),
    setup.autoLists === false ? withoutTypingRule(BulletList) : BulletList,
    setup.autoLists === false ? withoutTypingRule(OrderedList) : OrderedList,
    withBoundedStart(Table, undefined).configure({ resizable: false }),
    TableRow,
    TableHeader,
    TableCell,
    withBoundedStart(TaskList, TASK_GUARD, taskListExtent),
    TaskItem.configure({ nested: true }),
    LayoutImage.configure({ resolveSrc: setup.resolveSrc ?? ((src) => src) }),
    WikiLink.configure(setup.wiki ?? {}),
    withBoundedStart(Callout),
    withBoundedStart(MathBlock),
    MathInline,
    withBoundedStart(Details),
    DetailsSummary,
    withBoundedStart(FootnoteDef),
    FootnoteRef,
    ...(setup.emojiShortcodes === false ? [] : [EmojiShortcode, EmojiTyping]),
    // a private marked instance, the shared one would collect every editor's tokenizers
    // the option is typed as the global instance but any Marked instance works
    ReadableMarkdown.configure({ marked: new Marked() as unknown as typeof marked }),
    Extension.create({
      name: 'userShortcuts',
      priority: 200,
      addKeyboardShortcuts: () => setup.keys ?? {},
    }),
    ...(setup.placeholder ? [emptyNoteHint(setup.placeholder)] : []),
  ]
}

/** The hint in an empty note, only while the whole note is empty: an empty
 * line in the middle of a note stays blank. */
function emptyNoteHint(text: () => string) {
  return Placeholder.configure({
    placeholder: ({ editor }) => (editor.isEmpty ? text() : ''),
    showOnlyWhenEditable: true,
  })
}
