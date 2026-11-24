// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { backend } from '../backend'
import { htmlToMarkdown, parseHtml } from './html'
import { markdownImporter, inlineTags } from './importers/markdown'
import { parseDate, splitFrontMatter } from './markdown'
import { detect, fileAddress, prepare, reportMarkdown, settle, write } from './pipeline'
import { memorySource, resolvePath } from './source'
import { coreSource } from './testExport'
import { fileRef, noteRef, type ReportItem } from './types'

/** A new, empty vault in the core for each test. */
async function vault() {
  await backend.vaultReset()
  await backend.vaultCreate('plain', '')
  return backend
}

describe('paths', () => {
  it('resolves relative targets inside the export only', () => {
    expect(resolvePath('a/b/note.md', '../img/x%20y.png')).toBe('a/img/x y.png')
    expect(resolvePath('note.md', './pic.png?raw#top')).toBe('pic.png')
    expect(resolvePath('a/note.md', '/top.png')).toBe('top.png')
    expect(resolvePath('note.md', '../../out.png')).toBeNull()
    expect(resolvePath('note.md', 'https://example.com/a.png')).toBeNull()
    expect(resolvePath('note.md', '#heading')).toBeNull()
  })
})

describe('front matter and dates', () => {
  it('reads quoted values as the markdown export writes them', () => {
    // as the export writes it: title: "Say \"hi\" \\ back\nslash é"
    const yaml = [
      '---',
      String.raw`title: "Say \"hi\" \\ back\nslash é"`,
      `tags: ["a, b", 'it''s', plain]`,
      '---',
      '',
    ].join('\n')
    const { meta } = splitFrontMatter(yaml)
    expect(meta.title).toBe('Say "hi" \\ back\nslash é')
    expect(meta.tags).toEqual(['a, b', "it's", 'plain'])
  })

  it('reads values, inline lists and block lists', () => {
    const { meta, body } = splitFrontMatter(
      '---\ntitle: "Plan: v2"\ntags: [a, "b c"]\naliases:\n  - one\n  - two\ncreated: 2025-01-15\n---\nText',
    )
    expect(meta).toEqual({
      title: 'Plan: v2',
      tags: ['a', 'b c'],
      aliases: ['one', 'two'],
      created: '2025-01-15',
    })
    expect(body).toBe('Text')
  })

  it('keeps the lines of every key, nested maps and multi-line strings too', () => {
    const { fields } = splitFrontMatter(
      '---\ntitle: Plan\nsummary: |\n  Two lines\n  of text\nproject:\n  name: Garden\n---\nBody',
    )
    expect(fields).toEqual({
      title: 'title: Plan',
      summary: 'summary: |\n  Two lines\n  of text',
      project: 'project:\n  name: Garden',
    })
  })

  it('leaves a rule, a line and a rule in the body', () => {
    const text = '---\nShopping list\n---\nmilk'
    expect(splitFrontMatter(text)).toEqual({ meta: {}, fields: {}, body: text })
    const list = '---\n- milk\n- eggs\n---\nbread'
    expect(splitFrontMatter(list).body).toBe(list)
  })

  it('understands the date forms exports use', () => {
    expect(parseDate('20250115T103000Z')).toBe(Date.UTC(2025, 0, 15, 10, 30))
    expect(parseDate('2025-01-15T10:30:00Z')).toBe(Date.UTC(2025, 0, 15, 10, 30))
    expect(parseDate('2025-01-15 10:30:00Z')).toBe(Date.UTC(2025, 0, 15, 10, 30))
    expect(parseDate('2025-01-15')).toBe(new Date(2025, 0, 15).getTime())
    expect(parseDate('not a date')).toBeUndefined()
    expect(parseDate(undefined)).toBeUndefined()
  })

  it('finds inline tags but not headings, code, numbers or addresses', () => {
    const body =
      '# Heading\nsome #idea and #a/b, (#paren)\n`#code` see #123 and https://x.org/#frag\n```\n#nope\n```'
    expect(inlineTags(body)).toEqual(['idea', 'a/b', 'paren'])
  })
})

describe('settle', () => {
  it('makes titles unique, writes links and files out, reports what is missing', () => {
    const report: ReportItem[] = []
    const settled = settle(
      {
        folders: [
          { key: 'child', parent: 'top', name: 'Child' },
          { key: 'top', parent: null, name: 'Top' },
          { key: 'orphan', parent: 'gone', name: 'Orphan' },
        ],
        notes: [
          {
            key: 'a',
            folder: null,
            title: 'Plan',
            body: `see [[${noteRef('b', 'Plan')}#Goals|the other]]`,
            tags: ['#x', 'X', ' y '],
            files: [],
          },
          {
            key: 'b',
            folder: 'child',
            title: ' plan ',
            body: `[[${noteRef('nowhere', 'Lost note')}]] [[${noteRef('c', 'C')}]] ![p](${fileRef('a (1).png')}) ${fileRef('ghost.png')}`,
            tags: [],
            files: [
              { name: 'a (1).png', mime: 'image/png', entry: 'img/a (1).png' },
              { name: 'gone.png', mime: 'image/png', entry: 'missing.png' },
            ],
          },
          { key: 'c', folder: null, title: 'C# notes', body: '', tags: [], files: [] },
        ],
      },
      new Set(['img/a (1).png']),
      report,
    )
    const [a, b] = settled.notes
    expect(b?.title).toBe('plan 2')
    expect(a?.body).toBe('see [[plan 2#Goals|the other]]')
    expect(a?.tags).toEqual(['x', 'y'])
    expect(b?.body).toBe(
      `[[Lost note]] C# notes ![p](${fileAddress('a (1).png')}) attachments/ghost.png`,
    )
    expect(fileAddress('a (1).png')).toBe('attachments/a%20%281%29.png')
    expect(b?.files.map((f) => f.name)).toEqual(['a (1).png'])
    expect(settled.folders.map((f) => f.key)).toEqual(['top', 'orphan', 'child'])
    expect(settled.folders.find((f) => f.key === 'orphan')?.parent).toBeNull()
    expect(report.map((r) => r.kind).sort()).toEqual([
      'fileMissing',
      'fileMissing',
      'linkMissing',
      'simplified',
    ])
  })
})

const VAULT = {
  'Notes/Plan.md':
    '---\ntags: [work]\ncreated: 2024-03-01T09:00:00Z\n---\n# Big plan\nSee [[Ideas]], [[Ideas#Later|later]] and [the log](../Log%20book.md).\n![[diagram.png|300]] and ![photo](attachments/photo one.jpg)\n\n```\n[[not a link]]\n```\n#urgent',
  'Notes/Ideas.md': 'Just ideas. [[Nowhere]] ![[lost.png]] [a](report (final).pdf)',
  'Notes/report (final).pdf': new Uint8Array([9]),
  'Log book.md': 'Entries [web](https://example.com)',
  'Notes/attachments/photo one.jpg': new Uint8Array([1, 2, 3]),
  'assets/diagram.png': new Uint8Array([4, 5]),
  '.obsidian/app.json': '{}',
  '.trash/Old.md': 'old',
}

describe('a Ficelle export brought back', () => {
  it('keeps empty folders and leaves attached files out of the notes', async () => {
    const backend = await vault()
    const source = {
      ...memorySource('notes.zip', {
        'Plan.md': '# Plan',
        'attachments/Notes about it.md': 'an attached file',
        'attachments 2/Inside.md': '# Inside',
      }),
      dirs: ['Work', 'Work/Empty', 'attachments', 'attachments 2'],
    }
    const prepared = await prepare(source, markdownImporter, backend)
    expect(prepared.counts.notes).toBe(2)
    expect(prepared.model.folders.map((f) => f.key).sort()).toEqual([
      'Work',
      'Work/Empty',
      'attachments 2',
    ])
  })
})

describe('the markdown importer through the pipeline', () => {
  it('detects, previews and writes a folder with links, files, tags and dates', async () => {
    const backend = await vault()
    const source = await coreSource('Vault', VAULT, Date.UTC(2025, 5, 1))
    expect(detect(source, [markdownImporter])).toBe(markdownImporter)
    expect(detect(memorySource('x', { 'a.png': '' }), [markdownImporter])).toBeNull()

    const prepared = await prepare(source, markdownImporter, backend)
    expect(prepared.counts).toEqual({ notes: 3, folders: 1, files: 3, tags: 1, skipped: 0 })
    expect(prepared.dates).toEqual({ from: Date.UTC(2024, 2, 1, 9), to: Date.UTC(2025, 5, 1) })
    const plan = prepared.model.notes.find((n) => n.key.startsWith('Notes/Plan.md\n'))
    expect(plan?.title).toBe('Big plan')
    expect(plan?.body).toContain('See [[Ideas]], [[Ideas#Later|later]] and [[Log book|the log]].')
    expect(plan?.body).toContain('![diagram](attachments/diagram.png){width=300}')
    expect(plan?.body).toContain('![photo](attachments/photo%20one.jpg)')
    expect(plan?.body).toContain('[[not a link]]')
    const ideas = prepared.model.notes.find((n) => n.key.startsWith('Notes/Ideas.md\n'))
    expect(ideas?.body).toContain('[a](attachments/report%20%28final%29.pdf)')
    expect(prepared.report.map((r) => `${r.kind} ${r.detail}`).sort()).toEqual([
      'fileMissing lost.png',
      'linkMissing Nowhere',
    ])

    const done = await write(prepared, backend, { name: 'Markdown, today' })
    expect(done).toMatchObject({ notes: 3, skipped: 0, cancelled: false })
    const all = await backend.listNotes({ kind: 'all' }, 'titleAsc')
    expect(all.map((n) => n.title)).toEqual(['Big plan', 'Ideas', 'Log book'])
    expect(all.find((n) => n.title === 'Log book')?.folderId).toBe(done.root)
    const written = await backend.getNote(all[0]?.id ?? '')
    expect(written.folderId).not.toBe(done.root)
    const inNotes = await backend.listNotes({ kind: 'folder', id: written.folderId }, 'titleAsc')
    expect(inNotes.map((n) => n.title)).toEqual(['Big plan', 'Ideas'])
    // the folders it came in, as a tag of their own
    expect(written.tags).toEqual(['Markdown, today/Notes', 'work'])
    expect(written.createdAt).toBe(Date.UTC(2024, 2, 1, 9))
    expect((await backend.listAttachments(written.id)).map((a) => a.fileName).sort()).toEqual([
      'diagram.png',
      'photo one.jpg',
    ])

    // the same export again: nothing new
    const again = await prepare(source, markdownImporter, backend)
    expect(again.counts.notes).toBe(0)
    expect(again.counts.skipped).toBe(3)

    // another export with a note at the same path is not taken for this one
    const other = memorySource('Other', { 'Notes/Plan.md': 'Another plan', 'Notes/Ideas.md': '' })
    const fresh = await prepare(other, markdownImporter, backend)
    expect(fresh.counts).toMatchObject({ notes: 2, skipped: 0 })
  })

  it('reports the front matter it does not read', async () => {
    const backend = await vault()
    const source = memorySource('Notes', {
      'Plan.md':
        '---\ntitle: Plan\ndescription: Keep this sentence\nsummary: |\n  Two lines\n  of text\n---\nBody',
    })
    const prepared = await prepare(source, markdownImporter, backend)
    expect(prepared.model.notes[0]?.body).toBe('Body')
    expect(prepared.report.map((r) => `${r.kind} ${r.note}: ${r.detail}`).sort()).toEqual([
      'dropped Plan: description: Keep this sentence',
      'dropped Plan: summary: | Two lines of text',
    ])
  })

  it('keeps private use characters apart from its own markers', async () => {
    const backend = await vault()
    const text = 'Icons \uE000home\uE001x\uE002 and \uE003file\uE004 here'
    const prepared = await prepare(
      memorySource('Icons', { 'Icons.md': text }),
      markdownImporter,
      backend,
    )
    expect(prepared.model.notes[0]?.body).toBe(
      'Icons &#xE000;home&#xE001;x&#xE002; and &#xE003;file&#xE004; here',
    )
    expect(prepared.report).toEqual([])
  })

  it('knows notes an earlier version brought in by their path alone', async () => {
    const backend = await vault()
    const files = { 'a.md': 'one', 'b.md': 'two' }
    const first = await prepare(await coreSource('Old', files), markdownImporter, backend)
    const byPath = first.model.notes.map((n) => ({ ...n, key: n.key.split('\n')[0] ?? '' }))
    await write({ ...first, model: { ...first.model, notes: byPath } }, backend, { name: 'Old' })
    const again = await prepare(await coreSource('Old', files), markdownImporter, backend)
    expect(again.counts).toMatchObject({ notes: 0, skipped: 2 })
  })

  it('the inline tags option and a cancel between batches', async () => {
    const backend = await vault()
    const files: Record<string, string> = {}
    for (let i = 0; i < 250; i += 1) files[`n${i}.md`] = `note ${i}`
    const prepared = await prepare(await coreSource('Many', files), markdownImporter, backend)
    const stop = new AbortController()
    const done = await write(prepared, backend, {
      name: 'Many',
      signal: stop.signal,
      progress: (n) => n >= 100 && stop.abort(),
    })
    expect(done).toMatchObject({ notes: 100, cancelled: true })
    // what was written is known: the rest comes next time
    const rest = await prepare(await coreSource('Many', files), markdownImporter, backend)
    expect(rest.counts).toMatchObject({ notes: 150, skipped: 100 })
  })
})

describe('html to markdown', () => {
  it('goes through the editor schema', () => {
    const doc = parseHtml(
      `<html><head><title>x</title><style>p{}</style></head><body>
       <h2>Title</h2><p>Some <b>bold</b>&nbsp;and <i>italic</i> <a href="https://e.org">link</a></p>
       <ul><li>one</li><li>two</li></ul>
       <pre>let a = 1
let b = 2</pre>
       <script>alert(1)</script>
       <p><a data-wiki data-target="${noteRef('k', 'K')}" data-alias="other">other</a></p>
       <p><img src="${fileRef('pic.png')}" alt="pic"></p>
       <table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>
       </body></html>`,
    )
    const { markdown, dropped } = htmlToMarkdown(doc.body)
    expect(markdown).toContain('## Title')
    expect(markdown).toContain('Some **bold** and *italic* [link](https://e.org)')
    expect(markdown).toContain('- one\n- two')
    expect(markdown).toContain('```\nlet a = 1\nlet b = 2\n```')
    expect(markdown).toContain(`[[${noteRef('k', 'K')}|other]]`)
    expect(markdown).toContain(`![pic](${fileRef('pic.png')})`)
    expect(markdown).toMatch(/\| A +\| B +\|\n\| -+ \| -+ \|\n\| 1 +\| 2 +\|/)
    expect(markdown).not.toContain('alert')
    expect(dropped).toEqual(['script'])
  })
})

describe('report', () => {
  it('lists problems by kind with their note', () => {
    const text = reportMarkdown(
      {
        root: 'r',
        notes: 2,
        skipped: 0,
        cancelled: false,
        report: [
          { kind: 'linkMissing', note: 'A [x]', detail: 'B' },
          { kind: 'fileMissing', note: 'A', detail: 'c.png' },
          { kind: 'linkMissing', detail: 'D' },
        ],
      },
      {
        summary: '2 notes',
        kinds: { linkMissing: 'Links', fileMissing: 'Files' },
        none: 'All fine',
      },
    )
    expect(text).toBe(
      '2 notes\n\n## Links (2)\n\n- **A \\[x\\]**: B\n- D\n\n## Files (1)\n\n- **A**: c.png',
    )
  })
})

describe("this app's own export", () => {
  it('comes back with its folders, tags, pin, dates and files', async () => {
    const backend = await vault()
    const front = (title: string, pinned: boolean) =>
      `---
id: 01J0000000000000000000000${pinned ? 'A' : 'B'}
title: "${title}"
tags: ["garden"]
pinned: ${pinned}
created: 2024-03-01T09:00:00Z
updated: 2025-06-01T10:00:00Z
---

`
    const source = memorySource('Ficelle export', {
      'Projects/Garden/Tomato trellis plan.md': `${front('Tomato trellis plan', true)}![vines](attachments/vines.png)
`,
      'Projects/Garden/attachments/vines.png': new Uint8Array([1, 2, 3]),
      'Inbox.md': `${front('Inbox', false)}Buy twine.
`,
    })
    expect(detect(source, [markdownImporter])).toBe(markdownImporter)
    const prepared = await prepare(source, markdownImporter, backend)
    expect(prepared.counts).toMatchObject({ notes: 2, folders: 2, files: 1 })
    const plan = prepared.model.notes.find((n) => n.title === 'Tomato trellis plan')
    expect(plan?.tags).toEqual(['garden'])
    expect(plan?.pinned).toBe(true)
    expect(plan?.created).toBe(Date.UTC(2024, 2, 1, 9))
    expect(plan?.body).toContain('![vines](attachments/vines.png)')
    expect(prepared.report).toEqual([])
  })
})
