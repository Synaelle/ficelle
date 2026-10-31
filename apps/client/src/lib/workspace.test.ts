// SPDX-License-Identifier: AGPL-3.0-or-later
// Runs on the in-memory backend, the one a plain browser gets.
import { describe, expect, it } from 'vitest'
import { backend } from './backend'
import { workspace } from './workspace.svelte'

const ids = async () => (await backend.listNotes({ kind: 'all' }, 'updatedDesc')).map((n) => n.id)

describe('blank notes', () => {
  it('a new note left empty is gone once another is opened', async () => {
    await workspace.load()
    const [other] = await ids()
    const blank = await workspace.create()
    expect(await ids()).toContain(blank.id)
    await workspace.open(other ?? null)
    expect(await ids()).not.toContain(blank.id)
    expect(workspace.current?.id).toBe(other)
  })

  it('backing out to the list, or creating another, drops it too', async () => {
    const first = await workspace.create()
    const second = await workspace.create()
    expect(await ids()).not.toContain(first.id)
    await workspace.open(null)
    expect(await ids()).not.toContain(second.id)
  })

  it('a note with a title or text is kept', async () => {
    const titled = await workspace.create()
    workspace.edit({ title: 'Groceries' })
    await workspace.open(null)
    const written = await workspace.create()
    workspace.edit({ body: 'milk' })
    await workspace.open(null)
    expect(await ids()).toEqual(expect.arrayContaining([titled.id, written.id]))
  })

  it('opening the same note again keeps it even while blank', async () => {
    const blank = await workspace.create()
    await workspace.open(blank.id)
    expect(await ids()).toContain(blank.id)
    expect(workspace.current?.id).toBe(blank.id)
  })
})

describe('changes from another device', () => {
  it('with co-editing the open note takes them in place', async () => {
    workspace.coediting = () => true
    const note = await workspace.create({ title: 'Plan', body: 'one' })
    const before = workspace.revision
    // as if a pull rewrote it
    await backend.updateNote(note.id, { body: 'one\ntwo' })
    await workspace.remoteChanged()
    expect(workspace.current?.body).toBe('one\ntwo')
    expect(workspace.revision).toBe(before)
  })

  it('without co-editing the open note stays as shown', async () => {
    workspace.coediting = () => false
    const note = await workspace.create({ title: 'Quiet', body: 'one' })
    await backend.updateNote(note.id, { body: 'one\ntwo', title: 'Quiet' })
    await workspace.remoteChanged()
    expect(workspace.current?.body).toBe('one')
  })

  it('an edit being typed is saved from the text it started from', async () => {
    workspace.coediting = () => false
    const note = await workspace.create({ title: 'Draft', body: 'start' })
    const sent: unknown[] = []
    const update = backend.updateNote
    backend.updateNote = (id, change) => {
      sent.push(change)
      return update(id, change)
    }
    try {
      workspace.edit({ body: 'start and more' })
      await backend.updateNote(note.id, { body: 'changed elsewhere' })
      await workspace.remoteChanged()
      expect(workspace.current?.body).toBe('start and more')
      await workspace.flush()
      expect(sent.at(-1)).toEqual({ body: 'start and more', baseBody: 'start' })
    } finally {
      backend.updateNote = update
    }
  })

  it('a flush takes the typing the editor has not passed on yet', async () => {
    workspace.coediting = () => false
    const note = await workspace.create({ title: 'Typed', body: 'start' })
    // the editor still holds the last keys, in its pause before passing them
    workspace.serializeEditor = () => workspace.edit({ body: 'start, last keys' })
    try {
      await workspace.flush()
      expect((await backend.getNote(note.id)).body).toBe('start, last keys')
    } finally {
      workspace.serializeEditor = null
    }
  })

  it('with co-editing an edit waiting is saved first, then the note follows', async () => {
    workspace.coediting = () => true
    const note = await workspace.create({ title: 'Both', body: 'start' })
    workspace.edit({ body: 'start, mine' })
    await workspace.remoteChanged()
    expect(workspace.dirty).toBe(false)
    expect((await backend.getNote(note.id)).body).toBe('start, mine')
  })

  it('a note trashed elsewhere closes', async () => {
    const note = await workspace.create({ title: 'Old', body: 'x' })
    await backend.trashNote(note.id)
    await workspace.remoteChanged()
    expect(workspace.current).toBeNull()
  })
})
