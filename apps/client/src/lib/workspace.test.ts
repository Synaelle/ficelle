// SPDX-License-Identifier: AGPL-3.0-or-later
// Runs on the in-memory backend, the one a plain browser gets.
import { describe, expect, it } from 'vitest'
import { backend, type NoteChange } from './backend'
import { workspace } from './workspace.svelte'

/** A note made and opened, as the New note button does. */
async function create(initial: { title?: string; body?: string } = {}) {
  const note = await workspace.create(initial)
  if (!note) throw new Error('the open note could not be left')
  return note
}

const ids = async () => (await backend.listNotes({ kind: 'all' }, 'updatedDesc')).map((n) => n.id)

describe('blank notes', () => {
  it('a new note left empty is gone once another is opened', async () => {
    await workspace.load()
    const [other] = await ids()
    const blank = await create()
    expect(await ids()).toContain(blank.id)
    await workspace.open(other ?? null)
    expect(await ids()).not.toContain(blank.id)
    expect(workspace.current?.id).toBe(other)
  })

  it('backing out to the list, or creating another, drops it too', async () => {
    const first = await create()
    const second = await create()
    expect(await ids()).not.toContain(first.id)
    await workspace.open(null)
    expect(await ids()).not.toContain(second.id)
  })

  it('a note with a title or text is kept', async () => {
    const titled = await create()
    workspace.edit({ title: 'Groceries' })
    await workspace.open(null)
    const written = await create()
    workspace.edit({ body: 'milk' })
    await workspace.open(null)
    expect(await ids()).toEqual(expect.arrayContaining([titled.id, written.id]))
  })

  it('opening the same note again keeps it even while blank', async () => {
    const blank = await create()
    await workspace.open(blank.id)
    expect(await ids()).toContain(blank.id)
    expect(workspace.current?.id).toBe(blank.id)
  })
})

describe('changes from another device', () => {
  it('with co-editing the open note takes them in place', async () => {
    workspace.coediting = () => true
    const note = await create({ title: 'Plan', body: 'one' })
    const before = workspace.revision
    // as if a pull rewrote it
    await backend.updateNote(note.id, { body: 'one\ntwo' })
    await workspace.remoteChanged()
    expect(workspace.current?.body).toBe('one\ntwo')
    expect(workspace.revision).toBe(before)
  })

  it('without co-editing the open note stays as shown', async () => {
    workspace.coediting = () => false
    const note = await create({ title: 'Quiet', body: 'one' })
    await backend.updateNote(note.id, { body: 'one\ntwo', title: 'Quiet' })
    await workspace.remoteChanged()
    expect(workspace.current?.body).toBe('one')
  })

  it('an edit being typed is saved from the text it started from', async () => {
    workspace.coediting = () => false
    const note = await create({ title: 'Draft', body: 'start' })
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

  it('after a merged save the next one starts from the text sent, not again from the old one', async () => {
    workspace.coediting = () => false
    await create({ title: 'Draft', body: 'start' })
    const sent: NoteChange[] = []
    const update = backend.updateNote
    // another device wrote first: the save comes back merged
    backend.updateNote = async (id, change) => {
      sent.push(change)
      const saved = await update(id, { body: change.body })
      return sent.length === 1 ? { ...saved, body: `elsewhere ${change.body}` } : saved
    }
    try {
      workspace.edit({ body: 'start more' })
      await workspace.flush()
      workspace.edit({ body: 'start more again' })
      await workspace.flush()
      expect(sent.at(-1)).toEqual({ body: 'start more again', baseBody: 'start more' })
    } finally {
      backend.updateNote = update
    }
  })

  it('a failed save keeps the edit and the note, and saves it later', async () => {
    workspace.coediting = () => false
    const other = await create({ title: 'Other', body: '' })
    const note = await create({ title: 'Kept', body: 'start' })
    const update = backend.updateNote
    backend.updateNote = async () => {
      throw { kind: 'database', message: 'disk full' }
    }
    try {
      workspace.edit({ body: 'start, typed' })
      await workspace.flush()
      expect(workspace.unsaved).toBe(true)
      await workspace.open(other.id)
      expect(workspace.current?.id, 'the note stays open').toBe(note.id)
    } finally {
      backend.updateNote = update
    }
    await workspace.flush()
    expect(workspace.unsaved).toBe(false)
    expect((await backend.getNote(note.id)).body).toBe('start, typed')
    await workspace.open(other.id)
    expect(workspace.current?.id).toBe(other.id)
  })

  it('a flush takes the typing the editor has not passed on yet', async () => {
    workspace.coediting = () => false
    const note = await create({ title: 'Typed', body: 'start' })
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
    const note = await create({ title: 'Both', body: 'start' })
    workspace.edit({ body: 'start, mine' })
    await workspace.remoteChanged()
    expect(workspace.dirty).toBe(false)
    expect((await backend.getNote(note.id)).body).toBe('start, mine')
  })

  it('a note trashed elsewhere closes', async () => {
    const note = await create({ title: 'Old', body: 'x' })
    await backend.trashNote(note.id)
    await workspace.remoteChanged()
    expect(workspace.current).toBeNull()
  })

  it('an edit of a note trashed elsewhere never lands in the next note opened', async () => {
    workspace.coediting = () => false
    const other = await create({ title: 'Other', body: 'kept' })
    const note = await create({ title: 'Old', body: 'x' })
    workspace.edit({ title: 'Old, renamed' })
    await backend.trashNote(note.id)
    await workspace.remoteChanged()
    expect(workspace.current).toBeNull()
    expect(workspace.dirty).toBe(false)
    await workspace.open(other.id)
    workspace.edit({ body: 'kept, and more' })
    await workspace.flush()
    expect(await backend.getNote(other.id)).toMatchObject({
      title: 'Other',
      body: 'kept, and more',
    })
  })

  it('a title, tags and a pin changed elsewhere show in the open note', async () => {
    workspace.coediting = () => true
    const note = await create({ title: 'Plan', body: 'one' })
    await backend.updateNote(note.id, { title: 'Plan for May' })
    await backend.setPinned(note.id, true)
    await workspace.remoteChanged()
    expect(workspace.current).toMatchObject({ title: 'Plan for May', pinned: true })
    expect(workspace.notes.find((n) => n.id === note.id)).toMatchObject({
      title: 'Plan for May',
      pinned: true,
    })
    // tagged, it leaves the list of notes with no tag
    await backend.setNoteTags(note.id, ['garden'])
    await workspace.remoteChanged()
    expect(workspace.current?.tags).toEqual(['garden'])
    expect(workspace.tags.map((t) => t.name)).toContain('garden')
    expect(workspace.notes.some((n) => n.id === note.id)).toBe(false)
  })

  it('changes arriving together are taken in one after the other, the last one too', async () => {
    workspace.coediting = () => true
    const note = await create({ title: 'Plan', body: 'one' })
    const get = backend.getNote
    let reading = 0
    let most = 0
    backend.getNote = async (id) => {
      most = Math.max(most, ++reading)
      // the first read is slow: the store moves on while it runs
      if (most === 1 && reading === 1)
        await backend.updateNote(note.id, { body: 'one\ntwo\nthree' })
      const read = await get(id)
      reading--
      return read
    }
    try {
      await backend.updateNote(note.id, { body: 'one\ntwo' })
      const first = workspace.remoteChanged()
      const second = workspace.remoteChanged()
      await Promise.all([first, second])
      await first
    } finally {
      backend.getNote = get
    }
    expect(most).toBe(1)
    expect(workspace.current?.body).toBe('one\ntwo\nthree')
  })

  it('keys the editor still holds when a change arrives are kept, and merged by their save', async () => {
    workspace.coediting = () => true
    const note = await create({ title: 'Draft', body: 'start' })
    const sent: NoteChange[] = []
    const update = backend.updateNote
    // the store merges an edit made from an older text, as the real one does
    backend.updateNote = async (id, change) => {
      sent.push(change)
      const stored = (await backend.getNote(id)).body
      const merged = change.baseBody !== undefined && change.baseBody !== stored
      return update(id, merged ? { body: `${change.body} + ${stored}` } : change)
    }
    let held: string | null = null
    workspace.serializeEditor = () => {
      if (held !== null) workspace.edit({ body: held })
      held = null
    }
    try {
      await update(note.id, { body: 'elsewhere' })
      const get = backend.getNote
      // typed while the changed note is read
      backend.getNote = async (id) => {
        const read = await get(id)
        if (sent.length === 0) held = 'start, typed'
        return read
      }
      try {
        await workspace.remoteChanged()
      } finally {
        backend.getNote = get
      }
      expect(workspace.current?.body, 'the typing stays in the editor').toBe('start, typed')
      await workspace.flush()
      expect(sent).toEqual([{ body: 'start, typed', baseBody: 'start' }])
      expect(workspace.current?.body).toBe('start, typed + elsewhere')
      expect((await backend.getNote(note.id)).body).toBe('start, typed + elsewhere')
    } finally {
      backend.updateNote = update
      workspace.serializeEditor = null
    }
  })

  it('keys typed while a merged save was on its way are not replaced by the merge', async () => {
    workspace.coediting = () => true
    await create({ title: 'Draft', body: 'start' })
    const sent: NoteChange[] = []
    const update = backend.updateNote
    let held: string | null = null
    workspace.serializeEditor = () => {
      if (held !== null) workspace.edit({ body: held })
      held = null
    }
    backend.updateNote = async (id, change) => {
      sent.push(change)
      const saved = await update(id, { body: change.body })
      if (sent.length > 1) return saved
      // typed while the save was on its way
      held = `${change.body} and on`
      return { ...saved, body: `elsewhere ${change.body}` }
    }
    try {
      workspace.edit({ body: 'start more' })
      await workspace.flush()
      expect(workspace.current?.body).toBe('start more and on')
      expect(workspace.dirty).toBe(true)
      await workspace.flush()
      expect(sent.at(-1)).toEqual({ body: 'start more and on', baseBody: 'start more' })
    } finally {
      backend.updateNote = update
      workspace.serializeEditor = null
    }
  })

  it('saves go one at a time, each from the text the one before stored', async () => {
    workspace.coediting = () => false
    await create({ title: 'Draft', body: 'start' })
    const sent: NoteChange[] = []
    const update = backend.updateNote
    let running = 0
    let most = 0
    backend.updateNote = async (id, change) => {
      sent.push(change)
      most = Math.max(most, ++running)
      await new Promise((done) => setTimeout(done, 20))
      const saved = await update(id, change)
      running--
      return saved
    }
    try {
      workspace.edit({ body: 'start one' })
      const first = workspace.flush()
      workspace.edit({ body: 'start one two' })
      const second = workspace.flush()
      await Promise.all([first, second])
    } finally {
      backend.updateNote = update
    }
    expect(most).toBe(1)
    expect(sent).toEqual([
      { body: 'start one', baseBody: 'start' },
      { body: 'start one two', baseBody: 'start one' },
    ])
    expect(workspace.dirty).toBe(false)
  })

  it('a change arriving while a save is on its way shows once the save is done', async () => {
    workspace.coediting = () => true
    const note = await create({ title: 'Draft', body: 'start' })
    const update = backend.updateNote
    let release = () => {}
    const held = new Promise<void>((done) => (release = done))
    backend.updateNote = async (id, change) => {
      const saved = await update(id, change)
      await held
      return saved
    }
    try {
      workspace.edit({ body: 'start, mine' })
      const save = workspace.flush()
      // the other device's change lands after the save reached the store
      await update(note.id, { body: 'start, mine\ntheirs' })
      const followed = workspace.remoteChanged()
      release()
      await Promise.all([save, followed])
    } finally {
      backend.updateNote = update
    }
    expect(workspace.current?.body).toBe('start, mine\ntheirs')
    expect(workspace.dirty).toBe(false)
  })

  it('saving a title leaves the text the body started from alone', async () => {
    workspace.coediting = () => false
    const note = await create({ title: 'Draft', body: 'start' })
    const sent: NoteChange[] = []
    const update = backend.updateNote
    backend.updateNote = (id, change) => {
      sent.push(change)
      return update(id, change)
    }
    try {
      // another device rewrote the body, the open note still shows the old one
      await update(note.id, { body: 'changed elsewhere' })
      workspace.edit({ title: 'Draft two' })
      await workspace.flush()
      workspace.edit({ body: 'start and more' })
      await workspace.flush()
      expect(sent.at(-1)).toEqual({ body: 'start and more', baseBody: 'start' })
    } finally {
      backend.updateNote = update
    }
  })
})
