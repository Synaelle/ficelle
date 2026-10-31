// SPDX-License-Identifier: AGPL-3.0-or-later
// Touch gestures for the note list: swipe a row sideways, pull the list down
// from the top. Only touch starts them, mouse and pen keep their usual
// behaviour. Elements need `touch-action: pan-y` so sideways moves reach us
// while vertical scrolling stays with the browser.

import type { Action } from 'svelte/action'
import { tick } from './haptics'

/** Movement before a gesture picks a direction, under it a tap stays a tap. */
const SLOP = 10

export interface SwipeOptions {
  /** Rows that have nothing to swipe to, like notes in the trash. */
  disabled?: boolean
  /** Width of the actions revealed by a left swipe, the row rests there. */
  revealWidth: number
  /** Whether the actions are shown, the row slides to match. */
  open: boolean
  onOpen: () => void
  onClose: () => void
  /** A long swipe left, past most of the row. */
  onFull: () => void
  /** A swipe right. */
  onRight: () => void
}

/** Point past which a left swipe acts at once, as a share of the row width. */
const FULL_SHARE = 0.6
const RIGHT_DISTANCE = 80

/**
 * Slides the element's `.swipe-content` child with the finger. A swipe that
 * moved also swallows the click that follows it.
 */
export const swipeRow: Action<HTMLElement, SwipeOptions> = (node, initial) => {
  let options = initial
  let start: { x: number; y: number; id: number } | null = null
  let dx = 0
  let swiping = false
  let armed: 'full' | 'open' | 'right' | null = null
  let swallowClick = false

  const content = () => node.querySelector<HTMLElement>('.swipe-content')
  let hide: ReturnType<typeof setTimeout> | undefined
  const place = (x: number, animate: boolean) => {
    const el = content()
    if (!el) return
    el.style.transition = animate ? 'transform var(--dur-base) var(--ease-out)' : 'none'
    el.style.transform = x ? `translateX(${x}px)` : ''
    // the layers behind only show while the row moves or stays open, at rest
    // their colour would tint the anti aliased rounded corners
    clearTimeout(hide)
    if (x) node.classList.add('revealed')
    else hide = setTimeout(() => node.classList.remove('revealed'), animate ? 300 : 0)
  }
  const rest = () => place(options.open ? -options.revealWidth : 0, true)

  const down = (event: PointerEvent) => {
    if (options.disabled || event.pointerType !== 'touch' || !event.isPrimary) return
    swallowClick = false
    start = { x: event.clientX, y: event.clientY, id: event.pointerId }
    swiping = false
    armed = null
    dx = 0
  }

  const move = (event: PointerEvent) => {
    if (!start || event.pointerId !== start.id) return
    const x = event.clientX - start.x
    const y = event.clientY - start.y
    if (!swiping) {
      if (Math.abs(y) > SLOP && Math.abs(y) > Math.abs(x)) return (start = null)
      if (Math.abs(x) < SLOP) return
      swiping = true
      node.setPointerCapture(event.pointerId)
    }
    const base = options.open ? -options.revealWidth : 0
    dx = base + x
    const width = node.clientWidth
    const next =
      dx <= -width * FULL_SHARE
        ? 'full'
        : dx <= -options.revealWidth / 2
          ? 'open'
          : dx >= RIGHT_DISTANCE
            ? 'right'
            : null
    // a tick each time letting go would do something else
    if (next !== armed && (next === 'full' || next === 'right')) tick()
    armed = next
    place(Math.max(-width, Math.min(dx, RIGHT_DISTANCE * 1.5)), false)
  }

  const up = (event: PointerEvent) => {
    if (!start || event.pointerId !== start.id) return
    // a busy phone sends few moves for a quick flick, judge where it ended
    move(event)
    if (!start) return
    start = null
    if (!swiping) return
    swiping = false
    // Android sends no click after a moving touch, others do right away
    swallowClick = true
    setTimeout(() => (swallowClick = false), 400)
    if (armed === 'full') {
      place(-node.clientWidth, true)
      options.onFull()
    } else if (armed === 'right') {
      options.onRight()
      place(0, true)
    } else if (armed === 'open') {
      options.onOpen()
      place(-options.revealWidth, true)
    } else {
      options.onClose()
      place(0, true)
    }
  }

  const cancel = () => {
    start = null
    if (!swiping) return
    swiping = false
    rest()
  }

  const click = (event: MouseEvent) => {
    if (!swallowClick) return
    swallowClick = false
    event.preventDefault()
    event.stopPropagation()
  }

  node.addEventListener('pointerdown', down)
  node.addEventListener('pointermove', move)
  node.addEventListener('pointerup', up)
  node.addEventListener('pointercancel', cancel)
  node.addEventListener('click', click, true)
  return {
    update(next) {
      const changed = next.open !== options.open
      options = next
      if (changed && !swiping) rest()
    },
    destroy() {
      node.removeEventListener('pointerdown', down)
      node.removeEventListener('pointermove', move)
      node.removeEventListener('pointerup', up)
      node.removeEventListener('pointercancel', cancel)
      node.removeEventListener('click', click, true)
    },
  }
}

/** How far to pull before letting go acts, in px of finger travel. */
const PULL_DISTANCE = 70

/**
 * Pulling down while the scrolling child (`scroller`) is at the top stretches
 * the element, `onPull` runs when let go far enough. Touch events, not
 * pointer events: the browser takes vertical drags for scrolling and cancels
 * pointers, while a touch move can be kept from scrolling only when it pulls.
 */
export const pullDown: Action<HTMLElement, { scroller: string; onPull: () => void }> = (
  node,
  initial,
) => {
  let options = initial
  let start: { y: number; id: number } | null = null
  let pulled = 0
  let armed = false

  const set = (px: number, animate: boolean) => {
    node.style.transition = animate ? 'transform var(--dur-base) var(--ease-out)' : 'none'
    node.style.transform = px ? `translateY(${px}px)` : ''
    node.style.setProperty('--pull', String(Math.min(1, (px * 2) / PULL_DISTANCE)))
  }

  const begin = (event: TouchEvent) => {
    const touch = event.touches[0]
    const scroller = node.querySelector(options.scroller)
    if (event.touches.length !== 1 || !touch || (scroller && scroller.scrollTop > 0)) return
    start = { y: touch.clientY, id: touch.identifier }
    pulled = 0
    armed = false
  }

  const move = (event: TouchEvent) => {
    const touch = [...event.touches].find((t) => t.identifier === start?.id)
    if (!start || !touch) return
    const y = touch.clientY - start.y
    if (y <= 0) {
      // scrolling up into the list, leave it to the browser
      if (pulled > 0) set(0, true)
      start = null
      return
    }
    event.preventDefault()
    pulled = y
    const next = y >= PULL_DISTANCE
    if (next && !armed) tick()
    armed = next
    // half speed, the usual rubber band feel
    set(y / 2, false)
  }

  const end = () => {
    if (!start) return
    start = null
    set(0, true)
    if (armed && pulled >= PULL_DISTANCE) options.onPull()
  }

  node.addEventListener('touchstart', begin, { passive: true })
  // not passive, it must be able to stop the scroll while pulling
  node.addEventListener('touchmove', move, { passive: false })
  node.addEventListener('touchend', end)
  node.addEventListener('touchcancel', end)
  return {
    update: (next) => (options = next),
    destroy() {
      node.removeEventListener('touchstart', begin)
      node.removeEventListener('touchmove', move)
      node.removeEventListener('touchend', end)
      node.removeEventListener('touchcancel', end)
    },
  }
}
