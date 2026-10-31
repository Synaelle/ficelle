// SPDX-License-Identifier: AGPL-3.0-or-later
// Code block languages: the picker, the fence word kept as written, and
// colors only for a known language whose grammar has loaded.

import { Editor } from '@tiptap/core'
import { describe, expect, it } from 'vitest'
import { createExtensions } from './extensions'
import { knownLanguage } from './extensions/codeBlock'

function open(markdown: string) {
  const element = document.createElement('div')
  document.body.append(element)
  const editor = new Editor({
    element,
    extensions: createExtensions(),
    content: markdown,
    contentType: 'markdown',
  })
  return { editor, element }
}

/** Waits until the grammar has loaded and the block is redrawn. */
async function colored(element: HTMLElement) {
  for (let i = 0; i < 50; i++) {
    if (element.querySelector('[class*="hljs-"]')) return true
    await new Promise((resolve) => setTimeout(resolve, 20))
  }
  return false
}

describe('code block languages', () => {
  it('knows languages by name and alias, any case', () => {
    expect(knownLanguage('rust')?.id).toBe('rust')
    expect(knownLanguage('RS')?.id).toBe('rust')
    expect(knownLanguage('sh')?.id).toBe('bash')
    expect(knownLanguage('html')?.id).toBe('xml')
    expect(knownLanguage('haskell')).toBeUndefined()
    expect(knownLanguage('')).toBeUndefined()
  })

  it('keeps the fence word as written, known or not', () => {
    for (const word of ['js', 'haskell']) {
      const { editor } = open(`\`\`\`${word}\nx = 1\n\`\`\`\n`)
      expect(editor.getMarkdown()).toContain(`\`\`\`${word}\nx = 1`)
      editor.destroy()
    }
  })

  it('shows the language in the picker, an unknown word as itself', () => {
    const { editor, element } = open('```ts\nlet a = 1\n```\n\n```haskell\nmain = 1\n```\n')
    const pickers = [...element.querySelectorAll<HTMLSelectElement>('select.code-language')]
    expect(pickers.map((p) => p.value)).toEqual(['typescript', 'haskell'])
    editor.destroy()
  })

  it('picking a language writes it in the fence', () => {
    const { editor, element } = open('```\nSELECT 1;\n```\n')
    const picker = element.querySelector<HTMLSelectElement>('select.code-language')
    if (!picker) throw new Error('no language picker')
    expect(picker.value).toBe('')
    picker.value = 'sql'
    picker.dispatchEvent(new Event('change'))
    expect(editor.getMarkdown()).toContain('```sql\nSELECT 1;')
    editor.destroy()
  })

  it('colors a known language once its grammar loads, a plain block never', async () => {
    const plain = open('```\nconst x = "plain"\n```\n')
    await new Promise((resolve) => setTimeout(resolve, 200))
    expect(plain.element.querySelector('[class*="hljs-"]')).toBeNull()
    plain.editor.destroy()

    const js = open('```js\nconst x = "colored"\n```\n')
    expect(await colored(js.element)).toBe(true)
    expect(js.element.querySelector('.hljs-string')?.textContent).toBe('"colored"')
    // a redraw only: nothing to undo, nothing changed in the note
    expect(js.editor.can().undo()).toBe(false)
    js.editor.destroy()
  })
})
