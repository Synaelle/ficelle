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
  const doc = converter()
  doc.commands.setContent(root.innerHTML, { emitUpdate: false })
  const markdown = doc
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

/** A task list as the editor reads it, from items' HTML. */
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
    p.append(...item.content)
    li.append(p)
    list.append(li)
  }
  return list
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
