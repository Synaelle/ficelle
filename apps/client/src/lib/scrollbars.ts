// SPDX-License-Identifier: AGPL-3.0-or-later
// Linux: WebKitGTK draws the page's scrollbars (styles/app.css) without
// :hover or :active on their parts, and repaints them only when their element
// lays out again. So the scrollbar under the pointer is marked on its element,
// data-bar="hover", or "held" while pressed, and made to repaint; the styles
// read that mark instead.

type Mark = 'hover' | 'held'

let marked: HTMLElement | null = null
let held = false

/** Which scrollbar of `el` the point is on, if any, in page coordinates. */
export function onBar(
  box: { left: number; top: number; right: number; bottom: number },
  bars: { vertical: number; horizontal: number },
  borders: { right: number; bottom: number },
  x: number,
  y: number,
): boolean {
  const right = box.right - borders.right
  const bottom = box.bottom - borders.bottom
  const inside = x >= box.left && x <= right && y >= box.top && y <= bottom
  if (!inside) return false
  return (
    (bars.vertical > 0 && x >= right - bars.vertical) ||
    (bars.horizontal > 0 && y >= bottom - bars.horizontal)
  )
}

/** The nearest element with a scrollbar under the pointer, when the pointer is on that bar. */
function barUnder(event: PointerEvent): HTMLElement | null {
  for (let el = event.target instanceof Element ? event.target : null; el; el = el.parentElement) {
    if (!(el instanceof HTMLElement)) continue
    const style = getComputedStyle(el)
    const left = parseFloat(style.borderLeftWidth) || 0
    const right = parseFloat(style.borderRightWidth) || 0
    const top = parseFloat(style.borderTopWidth) || 0
    const bottom = parseFloat(style.borderBottomWidth) || 0
    const vertical = el.offsetWidth - el.clientWidth - left - right
    const horizontal = el.offsetHeight - el.clientHeight - top - bottom
    if (vertical <= 0 && horizontal <= 0) continue
    const r = el.getBoundingClientRect()
    const box = { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    // the pointer is over this element's content or its bar: never an outer bar
    return onBar(box, { vertical, horizontal }, { right, bottom }, event.clientX, event.clientY)
      ? el
      : null
  }
  return null
}

/** WebKitGTK paints a scrollbar's new style only after its element lays out. */
function repaint(el: HTMLElement) {
  const gutter = el.style.scrollbarGutter
  el.style.scrollbarGutter = 'stable'
  void el.offsetWidth
  el.style.scrollbarGutter = gutter
}

function mark(el: HTMLElement | null, state: Mark | null) {
  if (marked && marked !== el) {
    delete marked.dataset.bar
    repaint(marked)
    marked = null
  }
  // while a bar is held, what the pointer drifts over stays quiet (a pane's
  // divider right beside the bar)
  if (state === 'held') document.documentElement.dataset.barHeld = ''
  else delete document.documentElement.dataset.barHeld
  if (!el) return
  if (el.dataset.bar === state) return
  if (state) el.dataset.bar = state
  else delete el.dataset.bar
  repaint(el)
  marked = state ? el : null
}

/** Starts following the pointer over scrollbars, for the whole page. */
export function followScrollbars() {
  addEventListener(
    'pointermove',
    (event) => {
      if (held) return
      const el = barUnder(event)
      mark(el, el ? 'hover' : null)
    },
    { passive: true, capture: true },
  )
  addEventListener(
    'pointerdown',
    (event) => {
      if (event.button !== 0) return
      const el = barUnder(event)
      if (!el) return
      held = true
      mark(el, 'held')
    },
    { passive: true, capture: true },
  )
  const release = (event: PointerEvent) => {
    if (!held) return
    held = false
    const el = barUnder(event)
    mark(el ?? marked, el ? 'hover' : null)
  }
  addEventListener('pointerup', release, { passive: true, capture: true })
  addEventListener('pointercancel', release, { passive: true, capture: true })
  // the pointer left the window: nothing is under it any more
  document.documentElement.addEventListener('pointerleave', () => !held && mark(null, null))
}
