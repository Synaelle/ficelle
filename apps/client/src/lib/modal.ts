// SPDX-License-Identifier: AGPL-3.0-or-later
// Keyboard and screen reader behaviour of anything covering the page
// (settings, version history, dialogs): focus moves in when it opens, Tab
// and Shift+Tab stay inside, Esc closes it when it can close, and focus
// goes back to where it was. Only the top one reacts when they stack.

const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled]):not([type=hidden])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
  '[contenteditable="true"]',
].join(', ')

interface Options {
  /** Esc closes through this, leave it out when something else handles Esc. */
  onclose?: () => void
}

const stack: HTMLElement[] = []

export function modal(node: HTMLElement, options: Options = {}) {
  let onclose = options.onclose
  const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
  stack.push(node)

  const focusables = () =>
    [...node.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((e) => e.getClientRects().length > 0)

  // once its content is there, unless something inside already took focus.
  // Right away, not on the next frame: until then the one underneath pulled
  // focus back and kept the keys (a frame can come late, or not at all in a
  // window that is not drawn); the frame tries again once it is laid out.
  const enter = () => {
    if (stack.at(-1) !== node || !node.isConnected) return
    if (node.contains(document.activeElement)) return
    const target = node.querySelector<HTMLElement>('[data-autofocus]') ?? focusables()[0]
    if (target) target.focus()
    else {
      node.tabIndex = -1
      node.focus()
    }
  }
  queueMicrotask(enter)
  requestAnimationFrame(enter)

  let backwards = false
  function onkeydown(event: KeyboardEvent) {
    if (stack.at(-1) !== node) return
    if (event.key === 'Escape' && onclose) {
      event.preventDefault()
      event.stopPropagation()
      onclose()
    } else if (event.key === 'Tab') backwards = event.shiftKey
  }

  // Tab moves as usual and focus that lands outside wraps around. Guessing
  // the last stop beforehand fails on radio groups, which take one stop.
  function onfocusin(event: FocusEvent) {
    if (stack.at(-1) !== node || node.contains(event.target as Node)) return
    // a menu opened from inside sits outside, on top, and is part of it
    if (event.target instanceof Element && event.target.closest('[role=menu]')) return
    const list = focusables()
    const target = backwards ? list.at(-1) : list[0]
    ;(target ?? node).focus()
  }
  document.addEventListener('keydown', onkeydown, true)
  document.addEventListener('focusin', onfocusin, true)

  return {
    update(next: Options = {}) {
      onclose = next.onclose
    },
    destroy() {
      document.removeEventListener('keydown', onkeydown, true)
      document.removeEventListener('focusin', onfocusin, true)
      stack.splice(stack.lastIndexOf(node), 1)
      const lost = !document.activeElement || document.activeElement === document.body
      if (opener?.isConnected && (lost || node.contains(document.activeElement))) opener.focus()
    },
  }
}
