// SPDX-License-Identifier: AGPL-3.0-or-later
// Dragging notes from the list onto folders and the trash.

import type { Action } from 'svelte/action'
import type { Id } from './backend'

/** Private type, so file drops and text drags never look like a note. */
const NOTE_TYPE = 'application/x-app-note'

export function startNoteDrag(event: DragEvent, id: Id) {
  event.dataTransfer?.setData(NOTE_TYPE, id)
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move'
}

const carriesNote = (event: DragEvent) => event.dataTransfer?.types.includes(NOTE_TYPE) ?? false

/** Makes an element accept dropped notes, highlighted with `.drop-target`. */
export const noteDropTarget: Action<HTMLElement, (id: Id) => void> = (node, onDrop) => {
  let drop = onDrop
  const over = (event: DragEvent) => {
    if (!carriesNote(event)) return
    event.preventDefault()
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
    node.classList.add('drop-target')
  }
  const leave = () => node.classList.remove('drop-target')
  const dropped = (event: DragEvent) => {
    leave()
    const id = event.dataTransfer?.getData(NOTE_TYPE)
    if (!id) return
    event.preventDefault()
    drop(id)
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
