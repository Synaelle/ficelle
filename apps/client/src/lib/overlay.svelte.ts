// SPDX-License-Identifier: AGPL-3.0-or-later
// In-app confirmation, text prompt and context menu, drawn by Overlays.svelte.
// window.confirm and prompt must not be used: the tauri dialog plugin swaps
// them for async versions, and `if (confirm(...))` then read a Promise as
// yes, which deleted things without asking.

import { readText } from '@tauri-apps/plugin-clipboard-manager'
import { t } from './i18n.svelte'
import type { IconName } from './icons/sets'

export interface MenuItem {
  label: string
  icon?: IconName
  danger?: boolean
  disabled?: boolean
  /** A line between groups, see SEPARATOR. */
  separator?: boolean
  /** Shows a check mark, for the current choice in a list. */
  checked?: boolean
  run?: () => void
  /** Opens these items in place of the menu, for "Move to" and the like. */
  items?: MenuItem[]
}

/** A separator line between groups of items. */
export const SEPARATOR: MenuItem = { label: '', separator: true }

interface AskDialog {
  kind: 'ask'
  message: string
  confirmLabel: string
  danger: boolean
  resolve: (yes: boolean) => void
}

interface TextDialog {
  kind: 'text'
  message: string
  value: string
  /** Hides what is typed, for passwords. */
  secret: boolean
  /** Digits only, a number pad on phones: for a PIN. */
  digits?: boolean
  resolve: (value: string | null) => void
}

export type Dialog = AskDialog | TextDialog

interface Menu {
  x: number
  y: number
  items: MenuItem[]
  /** Previous levels, for the back item of a submenu. */
  parents: MenuItem[][]
  /** x is the right edge, for menus under a button at the end of a bar. */
  alignRight?: boolean
  /** Opened by a long touch: a sheet along the bottom with large rows,
   * not a small menu under the finger. */
  sheet?: boolean
}

class Overlay {
  dialog: Dialog | null = $state(null)
  menu: Menu | null = $state(null)
  /** Whether the last press was a finger, read when a context menu opens:
   * a long touch fires the same contextmenu event as a right click. */
  private touch = false

  constructor() {
    if (typeof window !== 'undefined')
      window.addEventListener(
        'pointerdown',
        (event) => {
          this.touch = event.pointerType === 'touch'
        },
        { capture: true, passive: true },
      )
  }

  /** The last press was a finger. */
  get byTouch() {
    return this.touch
  }

  /** Asks a yes or no question, resolves false when dismissed. */
  ask(message: string, options: { confirmLabel: string; danger?: boolean }): Promise<boolean> {
    this.answer(null)
    return new Promise((resolve) => {
      this.dialog = {
        kind: 'ask',
        message,
        confirmLabel: options.confirmLabel,
        danger: options.danger ?? false,
        resolve,
      }
    })
  }

  /** Asks for a line of text, resolves null when dismissed. */
  askText(message: string, value = ''): Promise<string | null> {
    this.answer(null)
    return new Promise((resolve) => {
      this.dialog = { kind: 'text', message, value, secret: false, resolve }
    })
  }

  /** Asks for the password, resolves null when dismissed. */
  askPassword(message: string): Promise<string | null> {
    this.answer(null)
    return new Promise((resolve) => {
      this.dialog = { kind: 'text', message, value: '', secret: true, resolve }
    })
  }

  /** Asks for a PIN, digits only, resolves null when dismissed. */
  askPin(message: string): Promise<string | null> {
    this.answer(null)
    return new Promise((resolve) => {
      this.dialog = { kind: 'text', message, value: '', secret: true, digits: true, resolve }
    })
  }

  /** Answers the open dialog and closes it. */
  answer(value: boolean | string | null) {
    const dialog = this.dialog
    this.dialog = null
    if (dialog?.kind === 'ask') dialog.resolve(value === true)
    else if (dialog?.kind === 'text') dialog.resolve(typeof value === 'string' ? value : null)
  }

  /** Opens a context menu at the pointer and stops the webview menu. */
  openMenu(event: MouseEvent, items: MenuItem[]) {
    event.preventDefault()
    event.stopPropagation()
    this.openMenuAt(event.clientX, event.clientY, items, false, this.touch)
  }

  /** Opens a menu under a button, aligned to its right edge when it fits. */
  openMenuBelow(button: HTMLElement, items: MenuItem[]) {
    const box = button.getBoundingClientRect()
    this.openMenuAt(box.right, box.bottom + 4, items, true)
  }

  /** Where focus was before the menu, it goes back there when the menu closes. */
  private menuOpener: HTMLElement | null = null

  private openMenuAt(x: number, y: number, items: MenuItem[], alignRight = false, sheet = false) {
    if (!this.menu) {
      const active = document.activeElement
      this.menuOpener = active instanceof HTMLElement && active !== document.body ? active : null
    }
    this.menu = { x, y, items, parents: [], alignRight, sheet }
  }

  enterSubmenu(item: MenuItem) {
    if (!this.menu || !item.items) return
    this.menu = {
      ...this.menu,
      parents: [...this.menu.parents, this.menu.items],
      items: item.items,
    }
  }

  leaveSubmenu() {
    if (!this.menu) return
    const parents = [...this.menu.parents]
    const items = parents.pop()
    if (items) this.menu = { ...this.menu, items, parents }
  }

  closeMenu() {
    if (!this.menu) return
    this.menu = null
    if (this.menuOpener?.isConnected) this.menuOpener.focus()
    this.menuOpener = null
  }
}

export const overlay = new Overlay()

/** Cut, copy and paste for any text field, the editor adds its own items. */
export function textItems(field: HTMLElement): MenuItem[] {
  const selected = (window.getSelection()?.toString() ?? '') !== '' || hasInputSelection(field)
  return [
    { label: t('menu.cut'), disabled: !selected, run: () => document.execCommand('cut') },
    { label: t('menu.copy'), disabled: !selected, run: () => document.execCommand('copy') },
    { label: t('menu.paste'), run: () => void paste(field) },
    SEPARATOR,
    { label: t('menu.selectAll'), run: () => selectAll(field) },
  ]
}

function hasInputSelection(field: HTMLElement) {
  return (
    (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) &&
    field.selectionStart !== field.selectionEnd
  )
}

async function paste(field: HTMLElement) {
  // the web clipboard api waits on a permission prompt the webview never shows
  const text = await readText().catch(() => '')
  field.focus()
  // insertText keeps undo working in inputs and in the editor
  if (text) document.execCommand('insertText', false, text)
}

function selectAll(field: HTMLElement) {
  field.focus()
  if (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) field.select()
  else document.execCommand('selectAll')
}
