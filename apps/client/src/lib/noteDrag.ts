// SPDX-License-Identifier: AGPL-3.0-or-later
// Dragging notes from the list onto tags and the trash, and tags onto tags.

import type { Action } from 'svelte/action'
import type { Id } from './backend'

/** Private types, so file drops and text drags never look like a note or a tag. */
const NOTE_TYPE = 'application/x-app-note'
const TAG_TYPE = 'application/x-app-tag'

function start(event: DragEvent, type: string, value: string) {
  event.dataTransfer?.setData(type, value)
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move'
}

export const startNoteDrag = (event: DragEvent, id: Id) => start(event, NOTE_TYPE, id)

/** A tag, by its name: dropped on another tag it goes inside it. */
export const startTagDrag = (event: DragEvent, name: string) => start(event, TAG_TYPE, name)

/** Makes an element accept dropped values of one type, highlighted with `.drop-target`. */
const dropTarget =
  (type: string): Action<HTMLElement, (value: string) => void> =>
  (node, onDrop) => {
    let drop = onDrop
    const over = (event: DragEvent) => {
      if (!(event.dataTransfer?.types.includes(type) ?? false)) return
      event.preventDefault()
      if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
      node.classList.add('drop-target')
    }
    const leave = () => node.classList.remove('drop-target')
    const dropped = (event: DragEvent) => {
      leave()
      const value = event.dataTransfer?.getData(type)
      if (!value) return
      event.preventDefault()
      drop(value)
    }
    node.addEventListener('dragover', over)
    node.addEventListener('dragleave', leave)
    node.addEventListener('drop', dropped)
    return {
      update: (next) => (drop = next),
      destroy() {
        node.removeEventListener('dragover', over)
        node.removeEventListener('dragleave', leave)
        node.removeEventListener('drop', dropped)
      },
    }
  }

/** Accepts dropped notes, by id. */
export const noteDropTarget = dropTarget(NOTE_TYPE)

/** Accepts dropped tags, by name. */
export const tagDropTarget = dropTarget(TAG_TYPE)
