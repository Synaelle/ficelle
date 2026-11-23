// SPDX-License-Identifier: AGPL-3.0-or-later
// Short messages at the bottom of the window that go away on their own,
// like a backup that failed. Each says what it is about, and a source shows
// a few at most, so one cannot cover the window.

import type { ShownError } from './errors'

export interface Toast {
  id: number
  /** Who says it, shown before the text. */
  from: string
  text: string
  /** An error's code, see lib/errors. */
  code?: string
  /** A button that undoes what the message reports. */
  action?: { label: string; run: () => void }
}

const SHOWN_MS = 5000
/** An error stays longer, time to copy its code. */
const ERROR_SHOWN_MS = 10000
const PER_SOURCE = 3

let next = 1

class Toasts {
  list: Toast[] = $state([])

  push(from: string, text: string | ShownError, action?: Toast['action']) {
    // over its share, a source's oldest message makes room: the newest is
    // the one that matters
    const mine = this.list.filter((t) => t.from === from)
    if (mine.length >= PER_SOURCE) this.dismiss((mine[0] as Toast).id)
    const toast: Toast =
      typeof text === 'string'
        ? { id: next++, from, text: text.slice(0, 300), action }
        : { id: next++, from, text: text.reason.slice(0, 300), code: text.code, action }
    this.list.push(toast)
    setTimeout(() => this.dismiss(toast.id), toast.code ? ERROR_SHOWN_MS : SHOWN_MS)
  }

  dismiss(id: number) {
    this.list = this.list.filter((t) => t.id !== id)
  }
}

export const toasts = new Toasts()
