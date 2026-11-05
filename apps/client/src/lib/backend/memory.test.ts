// SPDX-License-Identifier: AGPL-3.0-or-later
// The in-memory backend drives the browser preview, these checks keep it
// close to the core rules the ui relies on.

import { describe, expect, it } from 'vitest'
import { createMemoryBackend } from './memory'

describe('memory backend', () => {
  it('lists pinned notes first and hides the trash', async () => {
    const b = createMemoryBackend()
    const a = await b.createNote({ title: 'a' })
    const pinned = await b.createNote({ title: 'b', pinned: true })
    await b.trashNote(a.id)
    const all = await b.listNotes({ kind: 'all' }, 'updatedDesc')
    expect(all.map((n) => n.id)).toEqual([pinned.id])
    expect((await b.listNotes({ kind: 'trash' }, 'updatedDesc')).map((n) => n.id)).toEqual([a.id])
  })

  it('brings a trashed folder back with its notes, not those trashed before', async () => {
    const b = createMemoryBackend()
    const folder = await b.createFolder(null, 'f')
    const earlier = await b.createNote({ title: 'earlier', folderId: folder.id })
    const inside = await b.createNote({ title: 'inside', folderId: folder.id })
    await b.trashNote(earlier.id)
    await new Promise((resolve) => setTimeout(resolve, 2))
    await b.trashFolder(folder.id)
    expect((await b.listTrashedFolders()).map((f) => f.id)).toEqual([folder.id])
    await b.restoreFolder(folder.id)
    expect(await b.listTrashedFolders()).toEqual([])
    expect((await b.listFolders()).map((f) => f.id)).toEqual([folder.id])
    const trash = await b.listNotes({ kind: 'trash' }, 'updatedDesc')
    expect(trash.map((n) => n.id)).toEqual([earlier.id])
    const live = await b.listNotes({ kind: 'all' }, 'updatedDesc')
    expect(live.map((n) => n.id)).toEqual([inside.id])
  })

  it('finds backlinks and follows renames', async () => {
    const b = createMemoryBackend()
    const target = await b.createNote({ title: 'Garden log' })
    const linker = await b.createNote({ title: 'x', body: 'see [[garden log|the log]]' })
    expect((await b.backlinks(target.id)).map((n) => n.id)).toEqual([linker.id])
    await b.updateNote(target.id, { title: 'Garden journal' })
    expect((await b.getNote(linker.id)).body).toBe('see [[Garden journal|the log]]')
  })

  it('keeps attachment names unique per note', async () => {
    const b = createMemoryBackend()
    const note = await b.createNote({})
    const file = new File(['x'], 'vines.png', { type: 'image/png' })
    expect((await b.addAttachment(note.id, file)).fileName).toBe('vines.png')
    expect((await b.addAttachment(note.id, file)).fileName).toBe('vines 2.png')
  })
})
