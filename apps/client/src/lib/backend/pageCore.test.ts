// SPDX-License-Identifier: AGPL-3.0-or-later
// The core run in the page, the backend a plain browser gets: what the
// preview starts with, and the core's answers as the page reads them. Every
// test here shares its notes.

import { describe, expect, it } from 'vitest'
import { backend } from './index'

describe('the core in the page', () => {
  it('starts unlocked with the sample notes, pinned first', async () => {
    expect(await backend.vaultStatus()).toMatchObject({
      exists: true,
      unlocked: true,
      way: 'plain',
    })
    const notes = await backend.listNotes({ kind: 'all' }, 'updatedDesc')
    expect(notes.slice(0, 2).map((n) => n.title)).toEqual(['Tomato trellis plan', 'Pain au levain'])
    expect(notes).toHaveLength(6)
    const trellis = await backend.getNote(notes[0]?.id ?? '')
    expect(trellis.tags).toEqual(['Projects/Garden', 'todo'])
  })

  it("says the core's errors as the apps do", async () => {
    await expect(backend.getNote('0192f3a0-0000-7000-8000-000000000000')).rejects.toMatchObject({
      kind: 'notFound',
    })
  })

  it('keeps a file and gives its bytes back', async () => {
    const note = await backend.createNote({ title: 'Shed plan' })
    const file = new File([new Uint8Array([1, 2, 3])], 'shelf.png', { type: 'image/png' })
    const attachment = await backend.addAttachment(note.id, file)
    expect(attachment.fileName).toBe('shelf.png')
    expect(new Uint8Array(await backend.readAttachment(attachment.sha256))).toEqual(
      new Uint8Array([1, 2, 3]),
    )
  })
})
