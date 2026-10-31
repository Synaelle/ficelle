// SPDX-License-Identifier: AGPL-3.0-or-later
// Images placed like in a word processor. The markdown stays readable by
// other apps, layout goes in an attribute block they ignore:
//   ![vines](attachments/vines.png){width=320 wrap=left}
// wrap is inline (default), left, right, center or full. height is written
// only when the aspect ratio was unlocked with Shift while resizing.

import Image from '@tiptap/extension-image'
import type { Node as PMNode } from '@tiptap/pm/model'
import type { Editor } from '@tiptap/core'

export const WRAPS = ['inline', 'left', 'right', 'center', 'full'] as const

/** Window event after attachment urls changed: images look again. */
export const ATTACHMENTS_CHANGED = 'attachments-changed'
export type Wrap = (typeof WRAPS)[number]

const IMAGE = /^!\[([^\]]*)\]\(([^)\s]+)(?:\s+"([^"]*)")?\)(?:\{([^}\n]*)\})?/

/** `width=320 wrap=left` to an object. Unknown keys are ignored. */
export function parseLayout(block = ''): {
  width: number | null
  height: number | null
  wrap: Wrap
} {
  const pairs = Object.fromEntries(
    [...block.matchAll(/(\w+)=("[^"]*"|\S+)/g)].map(([, key, value]) => [
      key,
      (value ?? '').replace(/^"|"$/g, ''),
    ]),
  )
  const number = (text?: string) =>
    text && Number.isFinite(Number(text)) ? Math.round(Number(text)) : null
  const wrap = WRAPS.includes(pairs.wrap as Wrap) ? (pairs.wrap as Wrap) : 'inline'
  return { width: number(pairs.width), height: number(pairs.height), wrap }
}

export interface LayoutImageOptions {
  /** Turns a stored src like attachments/vines.png into a url the webview can load. */
  resolveSrc: (src: string) => string
}

const HANDLES = ['n', 'ne', 'e', 'se', 's', 'sw', 'w', 'nw'] as const

/**
 * Node view with eight resize handles. The aspect ratio is kept unless Shift
 * is held, then the height is stored too. Pointer events cover mouse, pen and
 * touch, pinch to resize arrives with the mobile phase.
 */
class ImageView {
  dom: HTMLElement
  private img: HTMLImageElement

  constructor(
    private node: PMNode,
    private editor: Editor,
    private getPos: () => number | undefined,
    private resolve: (src: string) => string,
  ) {
    this.dom = document.createElement('span')
    this.dom.className = 'image-view'
    this.img = document.createElement('img')
    this.img.draggable = false
    this.dom.append(this.img)
    for (const dir of HANDLES) {
      const handle = document.createElement('span')
      handle.className = 'handle'
      handle.dataset.dir = dir
      handle.addEventListener('pointerdown', (event) => this.startResize(event, dir))
      this.dom.append(handle)
    }
    this.render()
    window.addEventListener(ATTACHMENTS_CHANGED, this.refresh)
  }

  /** A file arrived or its preview changed: the url may be another now. */
  private refresh = () => {
    this.img.src = this.resolve((this.node.attrs.src as string | null) ?? '')
  }

  destroy() {
    window.removeEventListener(ATTACHMENTS_CHANGED, this.refresh)
  }

  private render() {
    const { src, alt, title, width, height, wrap } = this.node.attrs
    this.dom.dataset.wrap = wrap
    this.img.src = this.resolve(src ?? '')
    this.img.alt = alt ?? ''
    if (title) this.img.title = title
    else this.img.removeAttribute('title')
    this.img.style.width = wrap === 'full' ? '100%' : width ? `${width}px` : ''
    this.img.style.height = height && wrap !== 'full' ? `${height}px` : ''
  }

  private startResize(event: PointerEvent, dir: string) {
    event.preventDefault()
    event.stopPropagation()
    const handle = event.currentTarget as HTMLElement
    handle.setPointerCapture(event.pointerId)
    const start = {
      x: event.clientX,
      y: event.clientY,
      w: this.img.offsetWidth,
      h: this.img.offsetHeight,
    }
    const ratio = start.w / Math.max(1, start.h)
    // dragging a left handle to the left grows the image, so x is mirrored
    const sx = dir.includes('w') ? -1 : dir.includes('e') ? 1 : 0
    const sy = dir.includes('n') ? -1 : dir.includes('s') ? 1 : 0
    let size = { w: start.w, h: start.h, free: false }

    const move = (e: PointerEvent) => {
      const free = e.shiftKey
      let w = start.w + sx * (e.clientX - start.x)
      let h = start.h + sy * (e.clientY - start.y)
      if (!free) {
        // follow whichever axis moved more, the other one keeps the ratio
        if (sx === 0 || (sy !== 0 && Math.abs(h - start.h) * ratio > Math.abs(w - start.w)))
          w = h * ratio
        h = w / ratio
      }
      size = { w: Math.max(48, Math.round(w)), h: Math.max(32, Math.round(h)), free }
      this.img.style.width = `${size.w}px`
      this.img.style.height = free ? `${size.h}px` : ''
    }
    const end = () => {
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', end)
      const pos = this.getPos()
      if (pos === undefined) return
      this.editor
        .chain()
        .setNodeSelection(pos)
        .updateAttributes('image', { width: size.w, height: size.free ? size.h : null })
        .run()
    }
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', end)
  }

  update(node: PMNode) {
    if (node.type !== this.node.type) return false
    this.node = node
    this.render()
    return true
  }

  selectNode() {
    this.dom.classList.add('selected')
  }

  deselectNode() {
    this.dom.classList.remove('selected')
  }

  /** Handles own their pointer events, the editor must not see them. */
  stopEvent(event: Event) {
    return event.target instanceof HTMLElement && event.target.classList.contains('handle')
  }

  ignoreMutation() {
    return true
  }
}

export const LayoutImage = Image.extend<LayoutImageOptions>({
  addOptions() {
    return {
      ...this.parent?.(),
      inline: true,
      allowBase64: false,
      resolveSrc: (src: string) => src,
    }
  },

  addAttributes() {
    return {
      ...this.parent?.(),
      width: { default: null },
      height: { default: null },
      wrap: { default: 'inline' },
    }
  },

  addNodeView() {
    return ({ node, editor, getPos }) =>
      new ImageView(node, editor, getPos as () => number | undefined, this.options.resolveSrc)
  },

  markdownTokenizer: {
    name: 'image',
    level: 'inline',
    start: (src: string) => src.indexOf('!['),
    tokenize(src) {
      const match = IMAGE.exec(src)
      // plain images without a layout block go to marked's own image rule
      if (!match?.[4]) return undefined
      return {
        type: 'image',
        raw: match[0],
        text: match[1] ?? '',
        href: match[2] ?? '',
        title: match[3] ?? null,
        layout: parseLayout(match[4]),
      }
    },
  },

  parseMarkdown: (token, helpers) =>
    helpers.createNode('image', {
      src: token.href,
      alt: token.text,
      title: token.title ?? null,
      width: token.layout?.width ?? null,
      height: token.layout?.height ?? null,
      wrap: token.layout?.wrap ?? 'inline',
    }),

  renderMarkdown(node) {
    const { src, alt, title, width, height, wrap } = (node.attrs ?? {}) as Record<
      string,
      string | number | null
    >
    const layout = [
      width ? `width=${Math.round(Number(width))}` : '',
      height ? `height=${Math.round(Number(height))}` : '',
      wrap && wrap !== 'inline' ? `wrap=${wrap}` : '',
    ]
      .filter(Boolean)
      .join(' ')
    return `![${alt ?? ''}](${src}${title ? ` "${title}"` : ''})${layout ? `{${layout}}` : ''}`
  },
})
