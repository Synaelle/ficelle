// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { createMemoryBackend } from '../backend/memory'
import { evernoteImporter } from './importers/evernote'
import { dayOneImporter, opmlImporter, roamImporter, tomboyImporter } from './importers/extras'
import { htmlImporter } from './importers/html'
import { joplinImporter, parseItem } from './importers/joplin'
import { keepImporter } from './importers/keep'
import { IMPORTERS, notionImporter } from './index'
import { md5 } from './md5'
import { detect, prepare, write } from './pipeline'
import { memorySource } from './source'
import type { Importer, ImportSource, ReportItem } from './types'

async function run(source: ImportSource, importer: Importer, picked?: Importer) {
  const backend = createMemoryBackend()
  await backend.vaultCreate('plain', null)
  if (!picked) expect(detect(source, IMPORTERS)).toBe(importer)
  return { prepared: await prepare(source, importer, backend), backend }
}
const note = (p: { prepared: Awaited<ReturnType<typeof prepare>> }, title: string) => {
  const found = p.prepared.model.notes.find((n) => n.title === title)
  if (!found)
    throw new Error(`no note ${title}: ${p.prepared.model.notes.map((n) => n.title).join(', ')}`)
  return found
}
const kinds = (report: ReportItem[]) =>
  report.map((r) => `${r.kind} ${r.detail ?? ''}`.trim()).sort()
const encode = (text: string) => new TextEncoder().encode(text)
const b64 = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes))

describe('md5', () => {
  it('matches the reference values', () => {
    expect(md5(encode(''))).toBe('d41d8cd98f00b204e9800998ecf8427e')
    expect(md5(encode('abc'))).toBe('900150983cd24fb0d6963f7d28e17f72')
    expect(md5(encode('The quick brown fox jumps over the lazy dog'))).toBe(
      '9e107d9d372bb6826bd81d3542a419d6',
    )
    expect(md5(encode('a'.repeat(1000)))).toBe('cabe45dcc9ae5b66ba86600cca6b8ba8')
  })
})

describe('evernote', () => {
  it('reads notes, todos, files by md5, links by title, encrypted parts', async () => {
    const png = new Uint8Array([137, 80, 78, 71, 1, 2, 3])
    const pdf = encode('%PDF-1.4 tiny')
    const enex = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE en-export SYSTEM "http://xml.evernote.com/pub/evernote-export4.dtd">
<en-export export-date="20250101T000000Z" application="Evernote">
  <note>
    <title>Trip plan</title>
    <created>20240115T103000Z</created>
    <updated>20240116T090000Z</updated>
    <tag>travel</tag><tag>2024</tag>
    <note-attributes><source-url>https://example.com/trip</source-url></note-attributes>
    <content><![CDATA[<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE en-note SYSTEM "http://xml.evernote.com/pub/enml2.dtd">
<en-note><div><b>Pack</b> list</div><div><en-todo checked="true"/>passport</div><div><en-todo/>tickets</div>
<div><en-media type="image/png" hash="${md5(png)}"/></div><div><en-media type="application/pdf" hash="${md5(pdf)}"/></div>
<div>See <a href="evernote:///view/1/s1/abc/abc/">Packing tips</a> and <a href="evernote:///view/1/s1/x/x/">Gone note</a></div>
<en-crypt cipher="AES">secret</en-crypt><div><en-media type="image/png" hash="00000000000000000000000000000000"/></div></en-note>]]></content>
    <resource><data encoding="base64">${b64(png)}</data><mime>image/png</mime><resource-attributes><file-name>map.png</file-name></resource-attributes></resource>
    <resource><data encoding="base64">
${b64(pdf)}
</data><mime>application/pdf</mime></resource>
  </note>
  <note><title>Packing tips</title><created>20230101T000000Z</created><content><![CDATA[<en-note>Roll clothes</en-note>]]></content></note>
</en-export>`
    const r = await run(memorySource('Travel.enex', { 'Travel.enex': enex }), evernoteImporter)
    const trip = note(r, 'Trip plan')
    expect(trip.created).toBe(Date.UTC(2024, 0, 15, 10, 30))
    expect(trip.updated).toBe(Date.UTC(2024, 0, 16, 9))
    expect(trip.tags).toEqual(['travel', '2024'])
    expect(trip.body).toContain('**Pack** list')
    expect(trip.body).toContain('- [x] passport\n- [ ] tickets')
    expect(trip.body).toContain('![map](attachments/map.png)')
    expect(trip.body).toContain('[file.pdf](attachments/file.pdf)')
    expect(trip.body).toContain('See [[Packing tips]] and [[Gone note]]')
    expect(trip.body).toContain('<https://example.com/trip>')
    expect(trip.body).not.toContain('secret')
    expect(trip.files.map((f) => [f.name, f.bytes?.length])).toEqual([
      ['map.png', png.length],
      ['file.pdf', pdf.length],
    ])
    expect(kinds(r.prepared.report)).toEqual([
      'encrypted',
      'fileMissing image/png',
      'linkMissing Gone note',
    ])
    expect(r.prepared.model.folders).toEqual([])
  })

  it('makes a folder per notebook when there are several, and writes the files', async () => {
    const one = (title: string) =>
      `<en-export><note><title>${title}</title><content><![CDATA[<en-note>x</en-note>]]></content></note></en-export>`
    const r = await run(
      memorySource('export', {
        'Work.enex': one('A'),
        'Home.enex': one('B'),
        'Broken.enex': '<en-export><note>',
      }),
      evernoteImporter,
    )
    expect(r.prepared.model.folders.map((f) => f.name).sort()).toEqual(['Broken', 'Home', 'Work'])
    expect(note(r, 'A').folder).toBe('Work.enex')
    expect(kinds(r.prepared.report)).toEqual(['unreadable Broken.enex'])
    const written = await write(r.prepared, r.backend, { name: 'Evernote' })
    expect(written.notes).toBe(2)
  })
})

describe('joplin', () => {
  const id = (n: number) => n.toString(16).padStart(32, '0')
  const item = (title: string, body: string | null, meta: Record<string, string | number>) =>
    `${title}\n\n${body === null ? '' : `${body}\n\n`}${Object.entries(meta)
      .map(([k, v]) => `${k}: ${v}`)
      .join('\n')}`

  it('parses an item from its end', () => {
    expect(parseItem('Title\n\nBody line\nkey: not meta\n\nid: 1\ntype_: 1')).toEqual({
      title: 'Title',
      body: 'Body line\nkey: not meta',
      meta: { id: '1', type_: '1' },
    })
  })

  it('reads folders, notes, html notes, resources, tags, todos and the trash', async () => {
    const files = {
      [`${id(1)}.md`]: item('Projects', null, { id: id(1), parent_id: '', type_: 2 }),
      [`${id(2)}.md`]: item('Sub', null, { id: id(2), parent_id: id(1), type_: 2 }),
      [`${id(3)}.md`]: item(
        'Plan',
        `See [the list](:/${id(4)}) and ![chart](:/${id(5)})\n[gone](:/${id(9)})`,
        {
          id: id(3),
          parent_id: id(2),
          created_time: '2023-01-01T10:00:00.000Z',
          updated_time: '2023-01-02T10:00:00.000Z',
          user_created_time: '2022-12-31T10:00:00.000Z',
          user_updated_time: '2023-01-02T10:00:00.000Z',
          is_todo: 0,
          markup_language: 1,
          type_: 1,
        },
      ),
      [`${id(4)}.md`]: item('List', '<p>Milk <b>and</b> <img src=":/' + id(5) + '"></p>', {
        id: id(4),
        parent_id: id(1),
        is_todo: 1,
        todo_completed: 1700000000000,
        markup_language: 2,
        deleted_time: 0,
        type_: 1,
      }),
      [`${id(5)}.md`]: item('chart.png', null, {
        id: id(5),
        mime: 'image/png',
        file_extension: 'png',
        type_: 4,
      }),
      [`resources/${id(5)}.png`]: new Uint8Array([1, 2]),
      [`${id(6)}.md`]: item('work', null, { id: id(6), type_: 5 }),
      [`${id(7)}.md`]: item('', null, { id: id(7), note_id: id(3), tag_id: id(6), type_: 6 }),
      [`${id(8)}.md`]: item('Old', 'gone', {
        id: id(8),
        parent_id: '',
        deleted_time: '2024-01-01T00:00:00.000Z',
        type_: 1,
      }),
      [`${id(10)}.md`]: item('', '', { id: id(10), encryption_applied: 1, type_: 1 }),
    }
    const r = await run(memorySource('export.jex', files), joplinImporter)
    const plan = note(r, 'Plan')
    expect(plan.folder).toBe(id(2))
    expect(plan.tags).toEqual(['work'])
    expect(plan.created).toBe(Date.UTC(2022, 11, 31, 10))
    expect(plan.body).toBe('See [[List|the list]] and ![chart](attachments/chart.png)\ngone')
    const list = note(r, 'List')
    expect(list.tags).toEqual(['done'])
    expect(list.body).toBe('Milk **and** ![](attachments/chart.png)')
    expect(note(r, 'Old').trashed).toBe(true)
    expect(r.prepared.model.folders.map((f) => [f.name, f.parent])).toEqual([
      ['Projects', null],
      ['Sub', id(1)],
    ])
    expect(kinds(r.prepared.report)).toEqual([`encrypted ${id(10)}`, 'linkMissing gone'])
  })
})

describe('google keep', () => {
  it('reads text, checklists, labels, attachments with other extensions, links', async () => {
    const r = await run(
      memorySource('takeout.tgz', {
        'Takeout/Keep/Shopping.json': JSON.stringify({
          title: 'Shopping',
          listContent: [
            { text: 'eggs', isChecked: true },
            { text: 'milk', isChecked: false },
          ],
          createdTimestampUsec: 1700000000000000,
          userEditedTimestampUsec: 1700000500000000,
          isPinned: true,
          labels: [{ name: 'home' }],
        }),
        'Takeout/Keep/Idea.json': JSON.stringify({
          title: '',
          textContent: 'line one\nline two',
          isArchived: true,
          isTrashed: true,
          attachments: [{ filePath: 'photo.jpeg', mimetype: 'image/jpeg' }],
          annotations: [{ url: 'https://e.org', title: 'E' }],
        }),
        'Takeout/Keep/photo.jpg': new Uint8Array([9]),
        'Takeout/Keep/Labels.txt': 'home',
      }),
      keepImporter,
    )
    const shopping = note(r, 'Shopping')
    expect(shopping).toMatchObject({
      pinned: true,
      tags: ['home'],
      created: 1700000000000,
      updated: 1700000500000,
    })
    expect(shopping.body).toBe('- [x] eggs\n- [ ] milk')
    const idea = r.prepared.model.notes.find((n) => n.key === 'Idea.json')
    expect(idea?.body).toBe(
      'line one  \nline two\n\n![](attachments/photo.jpg)\n\n- [E](https://e.org)',
    )
    expect(idea).toMatchObject({ trashed: true, tags: ['archived'] })
  })
})

describe('html exports', () => {
  it('reads plain pages with links, pictures beside them and data pictures', async () => {
    const r = await run(
      memorySource('site', {
        'index.html':
          '<html><head><title>Home</title></head><body><h1>Home</h1><p>Go to <a href="sub/about.html">About us</a>, <a href="https://e.org">web</a>, <a href="gone.html">gone</a></p><img src="img/logo.png"><img src="data:image/png;base64,AQID"><video src="clip.mp4"></video><details><summary>More</summary><p>hidden</p></details><script>x()</script></body></html>',
        'sub/about.html': '<body><h1>About</h1><p>Text</p></body>',
        'img/logo.png': new Uint8Array([1]),
        'clip.mp4': new Uint8Array([2]),
      }),
      htmlImporter,
    )
    const home = note(r, 'Home')
    expect(home.body).toContain('Go to [[About|About us]], [web](https://e.org), gone')
    expect(home.body).toContain('![](attachments/logo.png)')
    expect(home.body).toContain('![](attachments/image.png)')
    expect(home.body).toContain('[clip.mp4](attachments/clip.mp4)')
    expect(home.body).toContain('**More**\n\nhidden')
    expect(home.files.map((f) => f.name)).toEqual(['logo.png', 'image.png', 'clip.mp4'])
    expect(note(r, 'About').folder).toBe('sub')
    expect(kinds(r.prepared.report)).toEqual(['dropped script', 'linkMissing gone.html'])
  })

  it('reads notion html with properties, to-dos and links by id', async () => {
    const id = (n: number) => n.toString(16).padStart(32, 'c')
    const page = (title: string, body: string, props = '') =>
      `<html><head><title>${title}</title></head><body><article><header><h1 class="page-title">${title}</h1>${props}</header><div class="page-body">${body}</div></article></body></html>`
    const r = await run(
      memorySource('Export.zip', {
        [`Home ${id(1)}.html`]: page(
          'Home page',
          `<p><a href="Home%20${id(1)}/Child%20${id(2)}.html">Child</a></p><ul class="to-do-list"><li><div class="checkbox checkbox-on"></div> <span>done thing</span></li><li><div class="checkbox checkbox-off"></div> <span>open thing</span></li></ul>`,
          '<table class="properties"><tr><th>Created</th><td><time>@March 3, 2024 9:15 AM</time></td></tr><tr><th>Tags</th><td><span class="selected-value">a</span><span class="selected-value">b c</span></td></tr></table>',
        ),
        [`Home ${id(1)}/Child ${id(2)}.html`]: page('Child', '<p>hi</p>'),
      }),
      notionImporter,
    )
    const home = note(r, 'Home page')
    expect(home.key).toBe(id(1))
    expect(home.tags).toEqual(['a', 'b c'])
    expect(home.created).toBe(new Date(2024, 2, 3, 9, 15).getTime())
    expect(home.body).toContain('- Created: March 3, 2024 9:15 AM')
    expect(home.body).toContain('[[Child]]')
    expect(home.body).toContain('- [x] done thing\n- [ ] open thing')
    expect(home.folder).toBe(`Home ${id(1)}`)
    expect(note(r, 'Child').folder).toBe(`Home ${id(1)}`)
  })
})

describe('extras', () => {
  it('reads an opml outline', async () => {
    const r = await run(
      memorySource('w.opml', {
        'w.opml':
          '<?xml version="1.0"?><opml version="2.0"><head><title>Workflowy</title></head><body><outline text="Plan"><outline text="&lt;b&gt;Step&lt;/b&gt; one" _note="a note" /><outline text="Done" _complete="true" /></outline></body></opml>',
      }),
      opmlImporter,
    )
    expect(note(r, 'Workflowy').body).toBe('- Plan\n  - Step one\n    a note\n  - [x] Done')
  })

  it('reads day one with its photos', async () => {
    const r = await run(
      memorySource('export.zip', {
        'Journal.json': JSON.stringify({
          entries: [
            {
              uuid: 'D1',
              text: '# Beach day\nSwam ![](dayone-moment://P1) ![](dayone-moment://P2)',
              creationDate: '2024-07-01T10:00:00Z',
              tags: ['summer'],
              starred: true,
              photos: [{ identifier: 'P1', md5: 'abc', type: 'jpeg' }],
            },
            { uuid: 'D2', text: 'Short line\nmore', creationDate: '2024-07-02T10:00:00Z' },
          ],
        }),
        'photos/abc.jpeg': new Uint8Array([1]),
      }),
      dayOneImporter,
    )
    const beach = note(r, 'Beach day')
    expect(beach).toMatchObject({
      key: 'D1',
      pinned: true,
      tags: ['summer'],
      created: Date.UTC(2024, 6, 1, 10),
    })
    expect(beach.body).toBe('Swam ![](attachments/abc.jpeg) ')
    expect(note(r, 'Short line').body).toBe('more')
    expect(kinds(r.prepared.report)).toEqual(['fileMissing P2'])
  })

  it('reads roam pages with block references and tasks', async () => {
    const r = await run(
      memorySource('graph.json', {
        'graph.json': JSON.stringify([
          {
            title: 'Garden',
            uid: 'pg1',
            'create-time': 1700000000000,
            children: [
              { string: '{{[[TODO]]}} dig #outdoor', uid: 'aaaaaaaaa' },
              {
                string: 'see [[Seeds]] __soft__ ^^bright^^',
                uid: 'bbbbbbbbb',
                children: [{ string: 'nested', uid: 'ccccccccc' }],
              },
            ],
          },
          {
            title: 'Seeds',
            children: [
              { string: 'ref ((aaaaaaaaa)) {{[[query]]: x}} #[[two words]]', uid: 'ddddddddd' },
            ],
          },
        ]),
      }),
      roamImporter,
    )
    const garden = note(r, 'Garden')
    expect(garden.body).toBe('- [ ] dig #outdoor\n- see [[Seeds]] *soft* bright\n  - nested')
    expect(garden.tags).toEqual(['outdoor'])
    expect(note(r, 'Seeds').body).toBe('- ref [ ] dig #outdoor  [[two words]]')
    expect(note(r, 'Seeds').tags).toEqual(['outdoor', 'two words'])
    expect(kinds(r.prepared.report)).toEqual(['dropped {{[[query]]: x}}'])
  })

  it('reads tomboy notes with notebooks and links', async () => {
    const tomboy = (
      title: string,
      content: string,
      tags = '',
    ) => `<?xml version="1.0" encoding="utf-8"?>
<note version="0.3" xmlns:link="http://beatniksoftware.com/tomboy/link" xmlns:size="http://beatniksoftware.com/tomboy/size" xmlns="http://beatniksoftware.com/tomboy">
<title>${title}</title><text xml:space="preserve"><note-content version="0.1">${title}
${content}</note-content></text>
<last-change-date>2024-02-01T10:00:00.0000000+00:00</last-change-date><create-date>2024-01-01T10:00:00.0000000+00:00</create-date>
<tags>${tags}</tags></note>`
    const r = await run(
      memorySource('notes', {
        'a.note': tomboy(
          'Ideas',
          '<bold>Big</bold> one, see <link:internal>Plans</link:internal> and <link:url>e.org</link:url>\n<list><list-item dir="ltr">first</list-item><list-item dir="ltr">second</list-item></list>',
          '<tag>system:notebook:Work</tag><tag>fun</tag>',
        ),
        'b.note': tomboy('Plans', 'later'),
        'c.note': tomboy('Template', 'x', '<tag>system:template</tag>'),
      }),
      tomboyImporter,
    )
    const ideas = note(r, 'Ideas')
    expect(ideas).toMatchObject({
      folder: 'Work',
      tags: ['fun'],
      created: Date.UTC(2024, 0, 1, 10),
    })
    expect(ideas.body).toBe(
      '**Big** one, see [[Plans]] and [e.org](https://e.org)\n\n- first\n- second',
    )
    expect(r.prepared.model.notes.map((n) => n.title).sort()).toEqual(['Ideas', 'Plans'])
    expect(r.prepared.model.folders).toEqual([{ key: 'Work', parent: null, name: 'Work' }])
  })
})
