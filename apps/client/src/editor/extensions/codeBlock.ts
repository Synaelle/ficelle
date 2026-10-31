// SPDX-License-Identifier: AGPL-3.0-or-later
// Code blocks with a language: a picker in the block's corner, and syntax
// colors from lowlight (highlight.js grammars). A grammar is loaded the first
// time a block uses its language, so only the core ships with the app. The
// language is the fence's word (```rust), kept as written; a block without one
// stays plain text, nothing is guessed.

import { CodeBlock } from '@tiptap/extension-code-block'
import { Plugin, PluginKey, type EditorState } from '@tiptap/pm/state'
import { Decoration, DecorationSet, type EditorView } from '@tiptap/pm/view'
import type { Node as PmNode } from '@tiptap/pm/model'
import { createLowlight } from 'lowlight'
import type { LanguageFn } from 'highlight.js'

type Loader = () => Promise<{ default: LanguageFn }>

/** The languages offered in the picker, in its order. */
export const LANGUAGES: { id: string; label: string; aliases?: string[]; load: Loader }[] = [
  {
    id: 'bash',
    label: 'Bash',
    aliases: ['sh', 'shell', 'zsh'],
    load: () => import('highlight.js/lib/languages/bash'),
  },
  { id: 'c', label: 'C', aliases: ['h'], load: () => import('highlight.js/lib/languages/c') },
  {
    id: 'cpp',
    label: 'C++',
    aliases: ['c++', 'cc', 'hpp'],
    load: () => import('highlight.js/lib/languages/cpp'),
  },
  {
    id: 'csharp',
    label: 'C#',
    aliases: ['cs', 'c#'],
    load: () => import('highlight.js/lib/languages/csharp'),
  },
  { id: 'css', label: 'CSS', load: () => import('highlight.js/lib/languages/css') },
  {
    id: 'diff',
    label: 'Diff',
    aliases: ['patch'],
    load: () => import('highlight.js/lib/languages/diff'),
  },
  {
    id: 'dockerfile',
    label: 'Dockerfile',
    aliases: ['docker'],
    load: () => import('highlight.js/lib/languages/dockerfile'),
  },
  {
    id: 'go',
    label: 'Go',
    aliases: ['golang'],
    load: () => import('highlight.js/lib/languages/go'),
  },
  {
    id: 'xml',
    label: 'HTML / XML',
    aliases: ['html', 'svg', 'svelte', 'vue'],
    load: () => import('highlight.js/lib/languages/xml'),
  },
  {
    id: 'ini',
    label: 'INI / TOML',
    aliases: ['toml', 'conf'],
    load: () => import('highlight.js/lib/languages/ini'),
  },
  { id: 'java', label: 'Java', load: () => import('highlight.js/lib/languages/java') },
  {
    id: 'javascript',
    label: 'JavaScript',
    aliases: ['js', 'jsx', 'mjs'],
    load: () => import('highlight.js/lib/languages/javascript'),
  },
  {
    id: 'json',
    label: 'JSON',
    aliases: ['jsonc'],
    load: () => import('highlight.js/lib/languages/json'),
  },
  {
    id: 'kotlin',
    label: 'Kotlin',
    aliases: ['kt', 'kts'],
    load: () => import('highlight.js/lib/languages/kotlin'),
  },
  { id: 'lua', label: 'Lua', load: () => import('highlight.js/lib/languages/lua') },
  {
    id: 'markdown',
    label: 'Markdown',
    aliases: ['md'],
    load: () => import('highlight.js/lib/languages/markdown'),
  },
  { id: 'php', label: 'PHP', load: () => import('highlight.js/lib/languages/php') },
  {
    id: 'powershell',
    label: 'PowerShell',
    aliases: ['ps1', 'pwsh'],
    load: () => import('highlight.js/lib/languages/powershell'),
  },
  {
    id: 'python',
    label: 'Python',
    aliases: ['py'],
    load: () => import('highlight.js/lib/languages/python'),
  },
  {
    id: 'ruby',
    label: 'Ruby',
    aliases: ['rb'],
    load: () => import('highlight.js/lib/languages/ruby'),
  },
  {
    id: 'rust',
    label: 'Rust',
    aliases: ['rs'],
    load: () => import('highlight.js/lib/languages/rust'),
  },
  { id: 'scss', label: 'SCSS', load: () => import('highlight.js/lib/languages/scss') },
  { id: 'sql', label: 'SQL', load: () => import('highlight.js/lib/languages/sql') },
  { id: 'swift', label: 'Swift', load: () => import('highlight.js/lib/languages/swift') },
  {
    id: 'typescript',
    label: 'TypeScript',
    aliases: ['ts', 'tsx'],
    load: () => import('highlight.js/lib/languages/typescript'),
  },
  {
    id: 'yaml',
    label: 'YAML',
    aliases: ['yml'],
    load: () => import('highlight.js/lib/languages/yaml'),
  },
]

const lowlight = createLowlight()
const byName = new Map<string, (typeof LANGUAGES)[number]>()
for (const language of LANGUAGES) {
  byName.set(language.id, language)
  for (const alias of language.aliases ?? []) byName.set(alias, language)
}
const loading = new Map<string, Promise<void>>()

/** The picker's language for a fence word, case and aliases aside. */
export function knownLanguage(word: string | null | undefined) {
  return word ? byName.get(word.trim().toLowerCase()) : undefined
}

/** Loads a language's grammar once; resolves when it can highlight. */
function load(language: (typeof LANGUAGES)[number]): Promise<void> {
  let pending = loading.get(language.id)
  if (!pending) {
    pending = language.load().then((module) => {
      lowlight.register({ [language.id]: module.default })
    })
    loading.set(language.id, pending)
  }
  return pending
}

interface Token {
  text: string
  classes: string[]
}

type HastNode = {
  type: string
  value?: string
  children?: HastNode[]
  properties?: { className?: string[] }
}

function flatten(nodes: HastNode[], classes: string[] = []): Token[] {
  return nodes.flatMap((node) => {
    const inner = [...classes, ...(node.properties?.className ?? [])]
    if (node.children) return flatten(node.children, inner)
    return [{ text: node.value ?? '', classes: inner }]
  })
}

const key = new PluginKey<DecorationSet>('codeColors')

/** Colors for every block whose grammar is loaded; asks for the others. */
function decorations(doc: PmNode, name: string, wanted: Set<(typeof LANGUAGES)[number]>) {
  const found: Decoration[] = []
  doc.descendants((node, pos) => {
    if (node.type.name !== name) return true
    const language = knownLanguage(node.attrs.language as string | null)
    if (!language) return false
    if (!lowlight.registered(language.id)) {
      wanted.add(language)
      return false
    }
    let from = pos + 1
    const tree = lowlight.highlight(language.id, node.textContent)
    for (const token of flatten(tree.children as HastNode[])) {
      const to = from + token.text.length
      if (token.classes.length)
        found.push(Decoration.inline(from, to, { class: token.classes.join(' ') }))
      from = to
    }
    return false
  })
  return DecorationSet.create(doc, found)
}

function colorsPlugin(name: string) {
  let wanted = new Set<(typeof LANGUAGES)[number]>()
  const compute = (state: EditorState) => {
    wanted = new Set()
    return decorations(state.doc, name, wanted)
  }
  return new Plugin<DecorationSet>({
    key,
    state: {
      init: (_, state) => compute(state),
      apply: (tr, old, _oldState, state) =>
        tr.docChanged || tr.getMeta(key) ? compute(state) : old.map(tr.mapping, tr.doc),
    },
    props: { decorations: (state) => key.getState(state) },
    view: (view: EditorView) => {
      let gone = false
      const fetchWanted = () => {
        for (const language of wanted)
          void load(language).then(() => {
            // a redraw only, nothing in the note changes
            if (!gone)
              view.dispatch(view.state.tr.setMeta(key, true).setMeta('addToHistory', false))
          })
      }
      fetchWanted()
      return {
        update: fetchWanted,
        destroy: () => {
          gone = true
        },
      }
    },
  })
}

export interface LanguageCodeBlockOptions {
  /** The picker's name for screen readers, read when the block is drawn. */
  pickerLabel: () => string
  /** The picker's word for no language. */
  plainLabel: () => string
}

/** The code block, with its language picker and colors. */
export const LanguageCodeBlock = CodeBlock.extend<LanguageCodeBlockOptions>({
  addOptions() {
    return {
      ...this.parent?.(),
      pickerLabel: () => 'Language',
      plainLabel: () => 'Plain text',
    }
  },
  addNodeView() {
    const { pickerLabel, plainLabel } = this.options
    return ({ node, getPos, editor }) => {
      const dom = document.createElement('div')
      dom.className = 'code-block'
      const picker = document.createElement('select')
      picker.className = 'code-language'
      picker.contentEditable = 'false'
      picker.setAttribute('aria-label', pickerLabel())
      const pre = document.createElement('pre')
      const code = document.createElement('code')
      // code is not prose, the spellchecker would underline most of it
      code.spellcheck = false
      pre.append(code)
      dom.append(picker, pre)

      let current = node
      const fill = () => {
        const word = (current.attrs.language as string | null) ?? ''
        const known = knownLanguage(word)
        const options = [['', plainLabel()], ...LANGUAGES.map((l) => [l.id, l.label])]
        // a word the picker does not know stays, and stays shown
        if (word && !known) options.push([word, word])
        picker.replaceChildren(
          ...options.map(([value, label]) => {
            const option = document.createElement('option')
            option.value = value ?? ''
            option.textContent = label ?? ''
            return option
          }),
        )
        picker.value = known ? known.id : word
        picker.disabled = !editor.isEditable
      }
      fill()
      picker.addEventListener('change', () => {
        const pos = typeof getPos === 'function' ? getPos() : undefined
        if (pos === undefined) return
        editor
          .chain()
          .command(({ tr }) => {
            tr.setNodeAttribute(pos, 'language', picker.value || null)
            return true
          })
          .run()
      })
      return {
        dom,
        contentDOM: code,
        update: (updated) => {
          if (updated.type !== current.type) return false
          const changed = updated.attrs.language !== current.attrs.language
          current = updated
          if (changed) fill()
          return true
        },
        // the picker's own clicks and keys are not editor input
        stopEvent: (event) => event.target === picker,
        ignoreMutation: (mutation) =>
          mutation.target === picker || picker.contains(mutation.target as Node),
      }
    }
  },
  addProseMirrorPlugins() {
    return [...(this.parent?.() ?? []), colorsPlugin(this.name)]
  },
})
