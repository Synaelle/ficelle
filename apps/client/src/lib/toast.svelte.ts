// SPDX-License-Identifier: AGPL-3.0-or-later
// Short messages at the bottom of the window that go away on their own,
// like a backup that failed. Each says what it is about, and a source shows
// a few at most, so one cannot cover the window.

export interface Toast {
  id: number
  /** Who says it, shown before the text. */
  from: string
  text: string
}

const SHOWN_MS = 5000
const PER_SOURCE = 3

let next = 1

class Toasts {
  list: Toast[] = $state([])

  push(from: string, text: string) {
    // over its share, a source's oldest message makes room: the newest is
    // the one that matters
    const mine = this.list.filter((t) => t.from === from)
    if (mine.length >= PER_SOURCE) this.dismiss((mine[0] as Toast).id)
    const toast = { id: next++, from, text: text.slice(0, 300) }
    this.list.push(toast)
    setTimeout(() => this.dismiss(toast.id), SHOWN_MS)
  }

  dismiss(id: number) {
    this.list = this.list.filter((t) => t.id !== id)
  }
}

export const toasts = new Toasts()
