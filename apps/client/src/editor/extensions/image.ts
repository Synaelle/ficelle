// SPDX-License-Identifier: AGPL-3.0-or-later
// Images placed like in a word processor. The markdown stays readable by
// other apps, layout goes in an attribute block they ignore:
//   ![vines](attachments/vines.png){width=320 wrap=left}
// wrap is inline (default), left, right, center or full. height is written
// only when the aspect ratio was unlocked with Shift while resizing. crop is
// what is cut from the top, right, bottom and left, in percent of the image,
// set by dragging a handle with Alt held: the file itself stays whole.
//   ![vines](attachments/vines.png){width=240 crop=10,0,5,20}

import Image from '@tiptap/extension-image'
import { Fragment, Slice, type Node as PMNode } from '@tiptap/pm/model'
import { NodeSelection } from '@tiptap/pm/state'
import { dropPoint } from '@tiptap/pm/transform'
import type { Editor } from '@tiptap/core'
import { pageZoom } from '../../lib/uiSize'

export const WRAPS = ['inline', 'left', 'right', 'center', 'full'] as const

/** Window event after attachment urls changed: images look again. */
export const ATTACHMENTS_CHANGED = 'attachments-changed'
export type Wrap = (typeof WRAPS)[number]

const IMAGE = /^!\[([^\]]*)\]\(([^)\s]+)(?:\s+"([^"]*)")?\)(?:\{([^}\n]*)\})?/

type Crop = [top: number, right: number, bottom: number, left: number]

/** `10,0,5,20` to fractions, null when nothing is cut or it makes no sense. */
export function parseCrop(text: unknown): Crop | null {
  if (typeof text !== 'string') return null
  const parts = text.split(',').map(Number)
  if (parts.length !== 4 || parts.some((n) => !Number.isFinite(n) || n < 0)) return null
  const [t, r, b, l] = parts.map((n) => n / 100) as Crop
  if (t + b >= 1 || l + r >= 1 || t + r + b + l === 0) return null
  return [t, r, b, l]
}

/** Fractions back to `10,0,5,20`, to a tenth of a percent. */
const cropText = (crop: Crop) => crop.map((n) => Math.round(n * 1000) / 10).join(',')

/** `width=320 wrap=left` to an object. Unknown keys are ignored. */
export function parseLayout(block = ''): {
  width: number | null
  height: number | null
  wrap: Wrap
  crop: string | null
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
  const crop = parseCrop(pairs.crop)
  return {
    width: number(pairs.width),
    height: number(pairs.height),
    wrap,
    crop: crop && cropText(crop),
  }
}

export interface LayoutImageOptions {
  /** Turns a stored src like attachments/vines.png into a url the webview can load. */
  resolveSrc: (src: string) => string
}

const HANDLES = ['n', 'ne', 'e', 'se', 's', 'sw', 'w', 'nw'] as const

/** A finger moves this far before a press on an image becomes a move. */
const MOVE_SLOP = 8
/** This close to the top or bottom of the note, a move scrolls it. */
const EDGE = 56

/** Runs `run` at most once per frame, however often it is asked. */
function perFrame(run: () => void): () => void {
  let queued = false
  return () => {
    if (queued) return
    queued = true
    requestAnimationFrame(() => {
      queued = false
      run()
    })
  }
}

/** The layout bar steps aside while an image is resized or moved. */
const busy = (on: boolean) => document.body.classList.toggle('image-busy', on)

/**
 * Node view with resize handles, only those that follow the pointer for the
 * image's layout (editor.css): an image held on its left grows to the right,
 * a centred one grows on both sides, so its handles move twice as far. The
 * aspect ratio is kept unless Shift is held, then the height is stored too.
 * With Alt held every handle shows, and dragging one cuts that side instead.
 * The size changes once per frame and is written to the note at the end.
 *
 * On touch screens a selected image moves with the finger: the browser's
 * drag and drop is for mice, Android's webview has none. With a mouse the
 * editor's own drag and drop moves it.
 */
class ImageView {
  dom: HTMLElement
  private img: HTMLImageElement
  private moving = false

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
    this.img.addEventListener('pointerdown', (event) => this.startMove(event))
    this.dom.append(this.img)
    for (const dir of HANDLES) {
      const handle = document.createElement('span')
      handle.className = 'handle'
      handle.dataset.dir = dir
      handle.addEventListener('pointerdown', (event) => this.startResize(event, dir))
      this.dom.append(handle)
    }
    // a cropped image is placed from its file's own size, known once loaded
    this.img.addEventListener('load', () => this.layout())
    this.render()
    window.addEventListener(ATTACHMENTS_CHANGED, this.refresh)
  }

  /** Alt shows every handle of the selected image, to crop from any side. */
  private altKey = (event: Event) => {
    const held = event instanceof KeyboardEvent && event.altKey
    this.dom.classList.toggle('cropping', held)
  }

  /** A file arrived or its preview changed: the url may be another now. */
  private refresh = () => {
    this.img.src = this.resolve((this.node.attrs.src as string | null) ?? '')
  }

  destroy() {
    window.removeEventListener(ATTACHMENTS_CHANGED, this.refresh)
    this.watchAlt(false)
  }

  private watchAlt(on: boolean) {
    for (const type of ['keydown', 'keyup', 'blur']) {
      if (on) window.addEventListener(type, this.altKey)
      else window.removeEventListener(type, this.altKey)
    }
    if (!on) this.dom.classList.remove('cropping')
  }

  private render() {
    const { src, alt, title, wrap } = this.node.attrs
    this.dom.dataset.wrap = wrap
    this.img.src = this.resolve(src ?? '')
    this.img.alt = alt ?? ''
    if (title) this.img.title = title
    else this.img.removeAttribute('title')
    this.layout()
  }

  private layout() {
    const { width, height } = this.node.attrs as { width: number | null; height: number | null }
    this.place(width, height, parseCrop(this.node.attrs.crop))
  }

  /**
   * Shows the image `w` wide on screen, and `h` high when its ratio is
   * unlocked. A crop draws the whole image larger, clipped to the part kept,
   * with negative margins so only that part takes room in the text.
   */
  private place(w: number | null, h: number | null, crop: Crop | null) {
    const style = this.img.style
    const full = this.node.attrs.wrap === 'full'
    const cut = full ? null : crop
    this.dom.classList.toggle('cropped', cut !== null)
    if (!cut) {
      style.width = full ? '100%' : w ? `${w}px` : ''
      style.height = h && !full ? `${h}px` : ''
      style.maxWidth = style.clipPath = style.margin = ''
      return
    }
    const { naturalWidth, naturalHeight } = this.img
    if (!naturalWidth) return
    const [t, r, b, l] = cut
    const shownW = w ?? naturalWidth * (1 - l - r)
    const wholeW = shownW / (1 - l - r)
    const wholeH = h ? h / (1 - t - b) : (wholeW * naturalHeight) / naturalWidth
    style.width = `${wholeW}px`
    style.height = h ? `${wholeH}px` : ''
    style.maxWidth = 'none'
    const pct = (n: number) => `${n * 100}%`
    style.clipPath = `inset(${pct(t)} ${pct(r)} ${pct(b)} ${pct(l)} round var(--radius-control))`
    style.margin = `${-t * wholeH}px ${-r * wholeW}px ${-b * wholeH}px ${-l * wholeW}px`
  }

  private startResize(event: PointerEvent, dir: string) {
    event.preventDefault()
    event.stopPropagation()
    const handle = event.currentTarget as HTMLElement
    handle.setPointerCapture(event.pointerId)
    if (event.altKey && this.node.attrs.wrap !== 'full') return this.startCrop(event, dir, handle)
    const crop = parseCrop(this.node.attrs.crop)
    // a cropped image's own box is larger than the part shown
    const shown = crop ? this.dom : this.img
    const start = {
      x: event.clientX,
      y: event.clientY,
      w: shown.offsetWidth,
      h: shown.offsetHeight,
    }
    const ratio = start.w / Math.max(1, start.h)
    // a centred image grows on both sides: its handle moves half the width
    const across = this.node.attrs.wrap === 'center' ? 2 : 1
    // dragging a left handle to the left grows the image, so x is mirrored
    const sx = (dir.includes('w') ? -1 : dir.includes('e') ? 1 : 0) * across
    const sy = dir.includes('n') ? -1 : dir.includes('s') ? 1 : 0
    // no wider than the column it sits in
    const room = this.dom.parentElement?.clientWidth || Infinity
    let size = { w: start.w, h: start.h, free: false }
    let last = event

    const apply = perFrame(() => {
      const free = last.shiftKey
      const zoom = pageZoom()
      let w = start.w + (sx * (last.clientX - start.x)) / zoom
      let h = start.h + (sy * (last.clientY - start.y)) / zoom
      if (!free) {
        // follow whichever axis moved more, the other one keeps the ratio
        if (sx === 0 || (sy !== 0 && Math.abs(h - start.h) * ratio > Math.abs(w - start.w)))
          w = h * ratio
        w = Math.min(w, room)
        h = w / ratio
      }
      size = {
        w: Math.max(48, Math.round(Math.min(w, room))),
        h: Math.max(32, Math.round(h)),
        free,
      }
      this.place(size.w, free ? size.h : null, crop)
    })
    const move = (e: PointerEvent) => {
      last = e
      apply()
    }
    const end = (e: PointerEvent) => {
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', end)
      handle.removeEventListener('pointercancel', end)
      this.dom.classList.remove('resizing')
      busy(false)
      // a cancelled touch leaves the image as it was
      if (e.type === 'pointercancel') return this.render()
      const pos = this.getPos()
      if (pos === undefined) return
      setImageLayout(this.editor, pos, { width: size.w, height: size.free ? size.h : null })
    }
    this.dom.classList.add('resizing')
    busy(true)
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', end)
    handle.addEventListener('pointercancel', end)
  }

  /** A handle dragged with Alt: its sides are cut, the image keeps its scale. */
  private startCrop(event: PointerEvent, dir: string, handle: HTMLElement) {
    const [t0, r0, b0, l0] = parseCrop(this.node.attrs.crop) ?? [0, 0, 0, 0]
    // the whole image as drawn now, the crop moves within it
    const wholeW = this.img.offsetWidth
    const wholeH = this.img.offsetHeight
    const free = this.node.attrs.height !== null
    const room = this.dom.parentElement?.clientWidth || Infinity
    // what stays is at least 48 by 32, and no wider than the column
    const minW = Math.min(1, 48 / wholeW)
    const minH = Math.min(1, 32 / wholeH)
    const maxW = Math.min(1, room / wholeW)
    const between = (n: number, low: number, high: number) => Math.min(high, Math.max(low, n))
    const start = { x: event.clientX, y: event.clientY }
    let crop: Crop = [t0, r0, b0, l0]
    let last = event

    const apply = perFrame(() => {
      const dx = (last.clientX - start.x) / wholeW
      const dy = (last.clientY - start.y) / wholeH
      let [t, r, b, l] = [t0, r0, b0, l0]
      if (dir.includes('n')) t = between(t0 + dy, 0, 1 - b0 - minH)
      if (dir.includes('s')) b = between(b0 - dy, 0, 1 - t0 - minH)
      if (dir.includes('w')) l = between(l0 + dx, Math.max(0, 1 - r0 - maxW), 1 - r0 - minW)
      if (dir.includes('e')) r = between(r0 - dx, Math.max(0, 1 - l0 - maxW), 1 - l0 - minW)
      crop = [t, r, b, l]
      this.place(wholeW * (1 - l - r), free ? wholeH * (1 - t - b) : null, crop)
    })
    const move = (e: PointerEvent) => {
      last = e
      apply()
    }
    const end = (e: PointerEvent) => {
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', end)
      handle.removeEventListener('pointercancel', end)
      this.dom.classList.remove('resizing')
      busy(false)
      if (e.type === 'pointercancel') return this.render()
      const pos = this.getPos()
      if (pos === undefined) return
      const [t, r, b, l] = crop
      const kept = parseCrop(cropText(crop))
      setImageLayout(this.editor, pos, {
        crop: kept && cropText(kept),
        width: Math.round(wholeW * (1 - l - r)),
        height: free ? Math.round(wholeH * (1 - t - b)) : null,
      })
    }
    this.dom.classList.add('resizing')
    busy(true)
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', end)
    handle.addEventListener('pointercancel', end)
  }

  /** A finger on a selected image: past a few pixels it carries the image. */
  private startMove(event: PointerEvent) {
    if (event.pointerType !== 'touch' || !this.editor.isEditable) return
    // a tap selects the image without focusing the text: no keyboard opening
    // and pushing the note around under the finger
    event.preventDefault()
    if (!this.dom.classList.contains('selected')) {
      this.img.addEventListener('pointerup', () => this.select(), { once: true })
      return
    }
    const view = this.editor.view
    const scroller = scrollParent(this.dom)
    const from = { x: event.clientX, y: event.clientY }
    this.img.setPointerCapture(event.pointerId)
    let ghost: HTMLElement | null = null
    let marker: HTMLElement | null = null
    let target: number | null = null
    let last = event
    let scrolling = 0

    const place = perFrame(() => {
      if (!ghost || !marker) return
      const zoom = pageZoom()
      ghost.style.transform = `translate(${last.clientX / zoom}px, ${last.clientY / zoom}px) translate(-50%, -50%)`
      target = this.dropAt(last.clientX, last.clientY)
      marker.hidden = target === null
      if (target === null) return
      const at = view.coordsAtPos(target)
      marker.style.transform = `translate(${at.left / zoom}px, ${at.top / zoom}px)`
      marker.style.height = `${Math.max(16, (at.bottom - at.top) / zoom)}px`
    })
    // near the top or bottom edge the note scrolls, a little every frame
    const edgeScroll = () => {
      if (!this.moving) return
      if (scroller) {
        const box = scroller.getBoundingClientRect()
        const y = last.clientY
        const step = y < box.top + EDGE ? -12 : y > box.bottom - EDGE ? 12 : 0
        if (step) {
          scroller.scrollTop += step
          place()
        }
      }
      scrolling = requestAnimationFrame(edgeScroll)
    }

    const begin = () => {
      this.moving = true
      busy(true)
      this.dom.classList.add('moving')
      const box = this.img.getBoundingClientRect()
      ghost = document.createElement('img')
      ghost.className = 'image-ghost'
      ghost.setAttribute('src', this.img.src)
      ghost.style.width = `${Math.min(box.width / pageZoom(), 160)}px`
      marker = document.createElement('span')
      marker.className = 'image-drop'
      marker.hidden = true
      document.body.append(ghost, marker)
      scrolling = requestAnimationFrame(edgeScroll)
    }

    const move = (e: PointerEvent) => {
      last = e
      if (!this.moving) {
        if (Math.hypot(e.clientX - from.x, e.clientY - from.y) < MOVE_SLOP) return
        begin()
      }
      place()
    }
    const end = (e: PointerEvent) => {
      this.img.removeEventListener('pointermove', move)
      this.img.removeEventListener('pointerup', end)
      this.img.removeEventListener('pointercancel', end)
      if (!this.moving) return
      cancelAnimationFrame(scrolling)
      ghost?.remove()
      marker?.remove()
      this.moving = false
      this.dom.classList.remove('moving')
      busy(false)
      if (e.type === 'pointerup' && target !== null) this.moveTo(target)
    }
    this.img.addEventListener('pointermove', move)
    this.img.addEventListener('pointerup', end)
    this.img.addEventListener('pointercancel', end)
  }

  private select() {
    const pos = this.getPos()
    if (pos === undefined) return
    const { tr } = this.editor.state
    this.editor.view.dispatch(tr.setSelection(NodeSelection.create(tr.doc, pos)))
  }

  /** Where the image would land under a point, never inside itself. */
  private dropAt(x: number, y: number): number | null {
    const pos = this.getPos()
    const found = this.editor.view.posAtCoords({ left: x, top: y })
    if (pos === undefined || !found) return null
    const slice = new Slice(Fragment.from(this.node), 0, 0)
    const doc = this.editor.state.doc
    const point = dropPoint(doc, found.pos, slice)
    const at = point === null ? null : wordEdge(doc, point)
    if (at === null || (at >= pos && at <= pos + this.node.nodeSize)) return null
    return at
  }

  private moveTo(target: number) {
    const pos = this.getPos()
    if (pos === undefined) return
    const tr = this.editor.state.tr.delete(pos, pos + this.node.nodeSize)
    const at = tr.mapping.map(target)
    tr.insert(at, this.node)
    tr.setSelection(NodeSelection.create(tr.doc, at))
    this.editor.view.dispatch(tr.scrollIntoView())
  }

  update(node: PMNode) {
    if (node.type !== this.node.type) return false
    this.node = node
    this.render()
    return true
  }

  selectNode() {
    this.dom.classList.add('selected')
    this.watchAlt(true)
  }

  deselectNode() {
    this.dom.classList.remove('selected')
    this.watchAlt(false)
  }

  /** Handles own their pointer events, the editor must not see them, nor
   * the touches carrying the image. */
  stopEvent(event: Event) {
    if (this.moving) return true
    return event.target instanceof HTMLElement && event.target.classList.contains('handle')
  }

  ignoreMutation() {
    return true
  }
}

/**
 * Changes an image's layout in place and keeps it selected. The editor's
 * updateAttributes replaces the node, which drops the selection to a caret
 * beside it, and the handles and layout bar with it.
 */
export function setImageLayout(editor: Editor, pos: number, attrs: Record<string, unknown>) {
  const tr = editor.state.tr
  for (const [key, value] of Object.entries(attrs)) tr.setNodeAttribute(pos, key, value)
  tr.setSelection(NodeSelection.create(tr.doc, pos))
  editor.view.dispatch(tr)
  // the keys go back to the note, but on a touch screen that is the keyboard
  // sliding up over the image
  if (!matchMedia('(pointer: coarse)').matches) editor.view.focus()
}

/** A position inside a word moved to the nearer end of it: an image dropped
 * in text never splits a word. */
export function wordEdge(doc: PMNode, at: number): number {
  const $at = doc.resolve(at)
  if (!$at.parent.isTextblock) return at
  const text = $at.parent.textBetween(0, $at.parent.content.size, undefined, '\ufffc')
  // every inline leaf (an image, a line break) counts one, as in positions
  let i = $at.parentOffset
  const inWord = (n: number) => /[\p{L}\p{N}_'-]/u.test(text[n] ?? '')
  if (!inWord(i - 1) || !inWord(i)) return at
  let left = i
  let right = i
  while (inWord(left - 1)) left--
  while (inWord(right)) right++
  i = i - left <= right - i ? left : right
  return $at.start() + i
}

/** The element that scrolls the note. */
function scrollParent(node: HTMLElement): HTMLElement | null {
  for (let el = node.parentElement; el; el = el.parentElement) {
    const { overflowY } = getComputedStyle(el)
    if ((overflowY === 'auto' || overflowY === 'scroll') && el.scrollHeight > el.clientHeight)
      return el
  }
  return null
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
      crop: { default: null },
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
      crop: token.layout?.crop ?? null,
    }),

  renderMarkdown(node) {
    const { src, alt, title, width, height, wrap, crop } = (node.attrs ?? {}) as Record<
      string,
      string | number | null
    >
    const layout = [
      width ? `width=${Math.round(Number(width))}` : '',
      height ? `height=${Math.round(Number(height))}` : '',
      wrap && wrap !== 'inline' ? `wrap=${wrap}` : '',
      crop ? `crop=${crop}` : '',
    ]
      .filter(Boolean)
      .join(' ')
    return `![${alt ?? ''}](${src}${title ? ` "${title}"` : ''})${layout ? `{${layout}}` : ''}`
  },
})
