// SPDX-License-Identifier: AGPL-3.0-or-later
// The editor schema. Used by the editor component and by the round trip
// tests, so both always agree on what markdown the app understands.

import {
  Extension,
  flattenExtensions,
  nodeInputRule,
  type AnyExtension,
  type Node,
} from '@tiptap/core'
import { Placeholder } from '@tiptap/extensions'
import { BulletList, OrderedList, TaskItem, TaskList } from '@tiptap/extension-list'
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table'
import StarterKit from '@tiptap/starter-kit'
import { Marked, type marked } from 'marked'
import { TypedArrows } from './arrows'
import { Callout, CalloutKeys } from './callout'
import { Details, DetailsSummary } from './details'
import { EmojiShortcode, EmojiTyping } from './emoji'
import { FindInNote } from './find'
import { FootnoteDef, FootnoteRef } from './footnote'
import { LanguageCodeBlock } from './codeBlock'
import { LayoutImage } from './image'
import {
  ReadableMarkdown,
  SafeParagraph,
  TASK_GUARD,
  taskListExtent,
  withAllCells,
  withBoundedStart,
  withEmptyItems,
  withShortRows,
} from './markdownFixes'
import { MathBlock, MathInline } from './math'
import { WikiLink, type WikiLinkOptions } from './wikiLink'
import { TypedHere } from '../patch'

export interface EditorSetup {
  resolveSrc?: (src: string) => string
  /** Said in place of a picture from the web, English by default. */
  webImage?: () => string
  wiki?: Partial<WikiLinkOptions>
  /** User key bindings in TipTap form ("Mod-b") for formatting commands. */
  keys?: Record<string, () => boolean>
  /** Turn "- " and "1. " at a line start into lists while typing. Default on. */
  autoLists?: boolean
  /** Turn "---" typed alone on a line into a horizontal line. Default on;
   * off, the dashes stay as typed and "***" or "___" still make one. */
  dashRule?: boolean
  /** Turn GitHub's `:name:` shortcodes into their emoji, typed or read.
   * Default on; off, they stay as written. */
  emojiShortcodes?: boolean
  /** What a typed -> becomes (→ by default), the other typed arrows
   * following; empty leaves them as typed. */
  arrow?: string
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

/** The kit's horizontal line made only from "*** " and "___ ", the dashes
 * staying as typed. */
function withoutDashRule(): Node {
  const rule = flattenExtensions([StarterKit]).find((part) => part.name === 'horizontalRule')
  return (rule as Node).extend({
    addInputRules() {
      return [nodeInputRule({ find: /^(?:___\s|\*\*\*\s)$/, type: this.type })]
    },
  })
}

export function createExtensions(setup: EditorSetup = {}): AnyExtension[] {
  return [
    StarterKit.configure({
      paragraph: false,
      codeBlock: false,
      bulletList: false,
      orderedList: false,
      // a web address typed out becomes a link, as it is when the note is
      // read again; bare words like example.com stay text, the note never
      // links them
      link: { openOnClick: false, shouldAutoLink: (url) => /^https?:\/\//i.test(url) },
      ...(setup.dashRule === false ? { horizontalRule: false as const } : {}),
    }),
    ...(setup.dashRule === false ? [withoutDashRule()] : []),
    SafeParagraph,
    LanguageCodeBlock.configure(
      setup.codeLabels
        ? { pickerLabel: setup.codeLabels.picker, plainLabel: setup.codeLabels.plain }
        : {},
    ),
    setup.autoLists === false ? withoutTypingRule(BulletList) : BulletList,
    withEmptyItems(setup.autoLists === false ? withoutTypingRule(OrderedList) : OrderedList),
    withShortRows(withAllCells(withBoundedStart(Table, undefined))).configure({ resizable: false }),
    TableRow,
    // one line of text per cell, all a Markdown table can hold: no table, list
    // or quote inside a cell, which the file could not keep
    TableHeader.extend({ content: 'paragraph' }),
    TableCell.extend({ content: 'paragraph' }),
    withBoundedStart(TaskList, TASK_GUARD, taskListExtent),
    TaskItem.configure({ nested: true }),
    LayoutImage.configure({
      resolveSrc: setup.resolveSrc ?? ((src) => src),
      ...(setup.webImage ? { webLabel: setup.webImage } : {}),
    }),
    WikiLink.configure(setup.wiki ?? {}),
    withBoundedStart(Callout),
    CalloutKeys,
    withBoundedStart(MathBlock),
    MathInline,
    withBoundedStart(Details),
    DetailsSummary,
    withBoundedStart(FootnoteDef),
    FootnoteRef,
    ...(setup.emojiShortcodes === false ? [] : [EmojiShortcode, EmojiTyping]),
    ...(setup.arrow === '' ? [] : [TypedArrows.configure({ arrow: setup.arrow ?? '→' })]),
    // a private marked instance, the shared one would collect every editor's tokenizers
    // the option is typed as the global instance but any Marked instance works
    ReadableMarkdown.configure({ marked: new Marked() as unknown as typeof marked }),
    Extension.create({
      name: 'userShortcuts',
      priority: 200,
      addKeyboardShortcuts: () => setup.keys ?? {},
    }),
    ...(setup.placeholder ? [emptyNoteHint(setup.placeholder)] : []),
    FindInNote,
    TypedHere,
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
