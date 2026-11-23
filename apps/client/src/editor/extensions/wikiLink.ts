// SPDX-License-Identifier: AGPL-3.0-or-later
// Wiki links: [[Note]], [[Note|label]], [[Note#Heading]], [[Note#Heading|label]].
// An inline atom, so the target cannot be half edited by accident. Typing
// [[ opens suggestions, the popup itself is WikiSuggest.svelte.

import { Node } from '@tiptap/core'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import Suggestion, { type SuggestionProps } from '@tiptap/suggestion'

const WIKI = /^\[\[([^\]|#\n]+)(?:#([^\]|\n]+))?(?:\|([^\]\n]+))?\]\]/

export interface WikiItem {
  title: string
  /** Set when completing [[Note#Heading. */
  heading?: string
}

export type SuggestEvent =
  | { phase: 'start' | 'update'; props: SuggestionProps<WikiItem> }
  | { phase: 'exit' }
  | { phase: 'key'; key: string }

export interface WikiLinkOptions {
  /** Notes matching what was typed after [[. */
  suggest: (query: string) => Promise<WikiItem[]>
  /** Drives the popup. Returns true when it handled a key. */
  onSuggest: (event: SuggestEvent) => boolean
}

export const wikiSuggestKey = new PluginKey('wikiSuggest')
/** Set while the caret is right after the space put behind a picked link. */
const pickedSpaceKey = new PluginKey<boolean>('wikiPickedSpace')

export const WikiLink = Node.create<WikiLinkOptions>({
  name: 'wikiLink',
  group: 'inline',
  inline: true,
  atom: true,
  selectable: true,

  addOptions: () => ({ suggest: async () => [], onSuggest: () => false }),

  addAttributes: () => ({
    target: { default: '' },
    heading: { default: null },
    alias: { default: null },
  }),

  parseHTML: () => [
    {
      tag: 'a[data-wiki]',
      getAttrs: (el) => ({
        target: el.dataset.target ?? '',
        heading: el.dataset.heading || null,
        alias: el.dataset.alias || null,
      }),
    },
  ],

  renderHTML({ node }) {
    const { target, heading, alias } = node.attrs as {
      target: string
      heading: string | null
      alias: string | null
    }
    const text = alias ?? (heading ? `${target} › ${heading}` : target)
    return [
      'a',
      {
        'data-wiki': '',
        'data-target': target,
        'data-heading': heading,
        'data-alias': alias,
        class: 'wiki-link',
        href: '#',
      },
      text,
    ]
  },

  markdownTokenizer: {
    name: 'wikiLink',
    level: 'inline',
    start: (src: string) => src.indexOf('[['),
    tokenize(src) {
      const match = WIKI.exec(src)
      if (!match) return undefined
      return {
        type: 'wikiLink',
        raw: match[0],
        target: (match[1] ?? '').trim(),
        heading: match[2]?.trim() ?? null,
        alias: match[3]?.trim() ?? null,
      }
    },
  },

  parseMarkdown: (token, helpers) =>
    helpers.createNode('wikiLink', {
      target: token.target,
      heading: token.heading,
      alias: token.alias,
    }),

  renderMarkdown: (node) => {
    const { target, heading, alias } = (node.attrs ?? {}) as Record<string, string | null>
    return `[[${target ?? ''}${heading ? `#${heading}` : ''}${alias ? `|${alias}` : ''}]]`
  },

  addProseMirrorPlugins() {
    const options = this.options
    return [
      Suggestion<WikiItem>({
        pluginKey: wikiSuggestKey,
        editor: this.editor,
        char: '[[',
        allowSpaces: true,
        items: ({ query }) => options.suggest(query),
        command: ({ editor, range, props }) =>
          editor
            .chain()
            .focus()
            .insertContentAt(range, [
              { type: 'wikiLink', attrs: { target: props.title, heading: props.heading ?? null } },
              { type: 'text', text: ' ' },
            ])
            .setMeta(pickedSpaceKey, true)
            .run(),
        render: () => ({
          onStart: (props) => void options.onSuggest({ phase: 'start', props }),
          onUpdate: (props) => void options.onSuggest({ phase: 'update', props }),
          onExit: () => void options.onSuggest({ phase: 'exit' }),
          onKeyDown: ({ event }) => options.onSuggest({ phase: 'key', key: event.key }),
        }),
      }),
      // the space after a picked link is the one typed next: typing it
      // again only walks past it, instead of leaving two
      new Plugin<boolean>({
        key: pickedSpaceKey,
        state: {
          init: () => false,
          apply: (tr, picked) =>
            (tr.getMeta(pickedSpaceKey) as boolean | undefined) ??
            (picked && !tr.docChanged && !tr.selectionSet),
        },
        props: {
          handleTextInput: (view, _from, _to, text) => {
            if (text !== ' ' || !pickedSpaceKey.getState(view.state)) return false
            view.dispatch(view.state.tr.setMeta(pickedSpaceKey, false))
            return true
          },
        },
      }),
    ]
  },
})
