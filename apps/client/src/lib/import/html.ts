// SPDX-License-Identifier: AGPL-3.0-or-later
// HTML to markdown through the editor's own schema: the HTML becomes an
// editor document and the editor's serializer writes it, so what comes in
// is what the editor shows and saves. Importers rewrite their own elements
// first (Evernote todos, Notion property tables) into plain HTML, links to
// notes as `<a data-wiki data-target>` and files as {@link fileRef}.

import { Editor } from '@tiptap/core'
import { createExtensions } from '../../editor/extensions'

/** Elements with no place in a note, their content dropped with them. */
const DROPPED =
  'script, style, noscript, iframe, object, embed, form, button, input, select, textarea, canvas, video, audio, svg, template'

let editor: Editor | null = null

/** One hidden editor for every conversion, made on first use. */
function converter(): Editor {
  editor ??= new Editor({
    element: document.createElement('div'),
    extensions: createExtensions({ autoLists: false }),
  })
  return editor
}

export function parseHtml(html: string): Document {
  return new DOMParser().parseFromString(html, 'text/html')
}

export interface Converted {
  markdown: string
  /** Tag names of what was dropped, once each. */
  dropped: string[]
}

/**
 * Converts an element's content (a page's body, an Evernote note) to
 * markdown. The element is changed: pass a copy to keep it.
 */
export function htmlToMarkdown(root: Element): Converted {
  const doc = root.ownerDocument
  // lists of checkboxes, before the form fields go
  for (const list of [...root.querySelectorAll('ul')]) {
    const items = [...list.children]
    const box = (li: Element) =>
      li.querySelector(':scope > input[type="checkbox"], :scope > p > input[type="checkbox"]')
    if (!items.length || !items.every((li) => li.localName === 'li' && box(li))) continue
    list.replaceWith(
      taskList(
        doc,
        items.map((li) => {
          const input = box(li) as HTMLInputElement
          input.remove()
          return { checked: input.hasAttribute('checked'), content: [...li.childNodes] }
        }),
      ),
    )
  }
  joinTaskLists(root)
  // a table with no header cells: its first row is the header, not an empty one
  for (const table of [...root.querySelectorAll('table')]) {
    if (table.querySelector('th')) continue
    for (const td of [...(table.querySelector('tr')?.children ?? [])]) {
      if (td.localName !== 'td') continue
      const th = doc.createElement('th')
      th.append(...td.childNodes)
      td.replaceWith(th)
    }
  }
  const dropped = new Set<string>()
  for (const el of root.querySelectorAll(DROPPED)) {
    // a stray form field is noise, not lost content
    if (!['input', 'button', 'select'].includes(el.localName)) dropped.add(el.localName)
    el.remove()
  }
  // line breaks in the source mean nothing in HTML outside pre
  for (const pre of root.querySelectorAll('pre')) {
    if (!pre.querySelector('code')) {
      const code = root.ownerDocument.createElement('code')
      code.textContent = pre.textContent
      pre.replaceChildren(code)
    }
  }
  const editor = converter()
  editor.commands.setContent(root.innerHTML, { emitUpdate: false })
  const markdown = editor
    .getMarkdown()
    .replace(/\u00A0/g, ' ')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
  return { markdown, dropped: [...dropped] }
}

/** Frees the hidden editor once an import is done. */
export function closeConverter() {
  editor?.destroy()
  editor = null
}

const BLOCK = 'div, p, ul, ol, table, blockquote, pre, h1, h2, h3, h4, h5, h6, hr'

/** A task list as the editor reads it, from items' HTML. An item's own text
 * goes in its paragraph and what is nested under it after: a block inside the
 * paragraph would split it, the text left out of the task. */
export function taskList(
  doc: Document,
  items: { checked: boolean; content: Node[] }[],
): HTMLElement {
  const list = doc.createElement('ul')
  list.setAttribute('data-type', 'taskList')
  for (const item of items) {
    const li = doc.createElement('li')
    li.setAttribute('data-type', 'taskItem')
    li.setAttribute('data-checked', String(item.checked))
    const p = doc.createElement('p')
    const nested: Node[] = []
    for (const node of item.content) {
      if (!(node instanceof Element) || !node.matches(BLOCK)) p.append(node)
      else if (node.matches('div, p') && !node.querySelector(BLOCK)) p.append(...node.childNodes)
      else if (node.textContent?.trim()) nested.push(node)
    }
    li.append(p, ...nested)
    list.append(li)
  }
  return list
}

/** Task lists one after the other made one, as they read. */
export function joinTaskLists(root: Element) {
  for (const list of [...root.querySelectorAll('ul[data-type="taskList"]')]) {
    const previous = list.previousElementSibling
    if (previous?.matches('ul[data-type="taskList"]')) {
      previous.append(...list.children)
      list.remove()
    }
  }
}

/** Bytes and type of a `data:` url, null when it is not one. */
export function dataUrl(url: string): { bytes: Uint8Array; mime: string } | null {
  const match = /^data:([^;,]*)(;base64)?,(.*)$/s.exec(url)
  if (!match) return null
  const mime = match[1] || 'application/octet-stream'
  const payload = match[3] ?? ''
  if (!match[2]) return { bytes: new TextEncoder().encode(decodeURIComponent(payload)), mime }
  return { bytes: base64Bytes(payload), mime }
}

/** Base64 to bytes, line breaks and spaces ignored. */
export function base64Bytes(text: string): Uint8Array {
  const clean = atob(text.replace(/\s+/g, ''))
  const bytes = new Uint8Array(clean.length)
  for (let i = 0; i < clean.length; i += 1) bytes[i] = clean.charCodeAt(i)
  return bytes
}
