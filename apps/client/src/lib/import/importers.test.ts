// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { createMemoryBackend } from '../backend/memory'
import { IMPORTERS, notionImporter } from './index'
import { bearImporter, bearTags } from './importers/bear'
import { lexicalToMarkdown, simplenoteImporter, standardNotesImporter } from './importers/json'
import { cleanPage, logseqImporter, pageName } from './importers/logseq'
import { markdownImporter } from './importers/markdown'
import { csvTable, notionProperties, parseCsv } from './importers/notion'
import { appleNotesImporter, obsidianImporter, zettlrImporter } from './importers/obsidian'
import { detect, plainSnippet, prepare } from './pipeline'
import { memorySource } from './source'
import type { Importer, ImportSource, ReportItem } from './types'

async function run(source: ImportSource, importer: Importer) {
  const backend = createMemoryBackend()
  await backend.vaultCreate('plain', null)
  expect(detect(source, IMPORTERS)).toBe(importer)
  return prepare(source, importer, backend)
}
const note = (p: Awaited<ReturnType<typeof run>>, title: string) => {
  const found = p.model.notes.find((n) => n.title === title)
  if (!found) throw new Error(`no note ${title}: ${p.model.notes.map((n) => n.title).join(', ')}`)
  return found
}
const kinds = (report: ReportItem[]) =>
  report.map((r) => `${r.kind} ${r.detail ?? ''}`.trim()).sort()

describe('obsidian', () => {
  it('reads a vault with its inline tags, leaving its settings and trash', async () => {
    const p = await run(
      memorySource('Vault', {
        '.obsidian/app.json': '{"attachmentFolderPath":"files"}',
        '.trash/Old.md': 'gone',
        'Daily/2025-01-15.md':
          '---\ncreated: 2025-01-15\naliases: [today]\n---\nMet #people/anna about [[Project X|X]] ![[shot.png]]',
        'Project X.md': '# Project X\n%%private%% and ==highlight== > [!note] callout\n#work',
        'files/shot.png': new Uint8Array([1]),
      }),
      obsidianImporter,
    )
    expect(p.model.notes.map((n) => n.title).sort()).toEqual(['2025-01-15', 'Project X'])
    const daily = note(p, '2025-01-15')
    expect(daily.tags).toEqual(['people/anna'])
    expect(daily.body).toContain('[[Project X|X]]')
    expect(daily.body).toContain('![shot](attachments/shot.png)')
    expect(note(p, 'Project X').tags).toEqual(['work'])
    expect(p.model.folders.map((f) => f.name)).toEqual(['Daily'])
  })
})

describe('zettlr', () => {
  it('finds notes by their id and dates them by it', async () => {
    const p = await run(
      memorySource('Zettelkasten', {
        '.ztr-directory': '{}',
        '20250115103000 Atomic notes.md':
          '---\ntitle: Atomic notes\nkeywords: [method]\n---\nSee [[20250116090000]].',
        '20250116090000 Links.md': 'Links matter #idea',
      }),
      zettlrImporter,
    )
    const atomic = note(p, 'Atomic notes')
    expect(atomic.tags).toEqual(['method'])
    expect(atomic.body).toBe('See [[20250116090000 Links]].')
    expect(atomic.created).toBe(new Date(2025, 0, 15, 10, 30).getTime())
    expect(note(p, '20250116090000 Links').tags).toEqual(['idea'])
  })
})

describe('logseq', () => {
  it('names pages, reads properties, tasks, tags and block references', async () => {
    const p = await run(
      memorySource('graph', {
        'logseq/config.edn': '{}',
        'logseq/bak/pages/x.md': 'backup',
        'pages/Projects___Garden.md':
          'title:: Projects/Garden\ntags:: outdoor, [[home life]]\n\n- TODO dig the bed\n- DONE buy seeds\n  id:: 6512bd43-d9ca-4e4e-b2e1-6f7a5c3a9b01\n- See [[Oct 27th, 2025]] and #[[two words]]\n- {{query (todo now)}}\n- ![bed](../assets/bed.png)',
        'journals/2025_10_27.md':
          '- Planted things ((6512bd43-d9ca-4e4e-b2e1-6f7a5c3a9b01))\n- lost ((00000000-0000-0000-0000-000000000000))\n- {{embed [[Projects/Garden]]}}',
        'assets/bed.png': new Uint8Array([7]),
        'pages/notes.org': '* org',
      }),
      logseqImporter,
    )
    const garden = note(p, 'Projects/Garden')
    expect(garden.tags).toEqual(['outdoor', 'home life', 'two words'])
    expect(garden.body).toContain('- [ ] dig the bed')
    expect(garden.body).toContain('- [x] buy seeds')
    expect(garden.body).not.toContain('id::')
    expect(garden.body).toContain('[[2025-10-27]] and [[two words]]')
    expect(garden.body).toContain('![bed](attachments/bed.png)')
    const day = note(p, '2025-10-27')
    expect(day.folder).toBe('journals')
    expect(day.created).toBe(new Date(2025, 9, 27).getTime())
    expect(day.body).toContain('Planted things buy seeds')
    expect(day.body).toContain('[[Projects/Garden]]')
    expect(kinds(p.report)).toEqual([
      'dropped {{query (todo now)}}',
      'linkMissing ((00000000-0000-0000-0000-000000000000))',
      'linkMissing two words',
      'unreadable pages/notes.org',
    ])
    expect(pageName('pages/a%3Fb___c.md')).toBe('a?b/c')
  })

  it('keeps text it does not know', () => {
    const report: ReportItem[] = []
    const r = { add: (i: ReportItem) => report.push(i), progress: () => {} }
    expect(cleanPage('- NOW write\n- CANCELED old plan', 't', new Map(), r).text).toBe(
      '- [ ] write\n- ~~old plan~~',
    )
  })
})

describe('bear', () => {
  it('reads text bundles with their info, files and tags', async () => {
    const info = (extra: object) =>
      JSON.stringify({
        'net.shinyfrog.bear': {
          creationDate: 700000000,
          modificationDate: '2024-05-01T10:00:00Z',
          ...extra,
        },
      })
    const p = await run(
      memorySource('Bear Notes.bear2bk', {
        'Recipes.textbundle/text.markdown':
          '# Recipes\nBread ~underlined~ #food/baking #two words# and [[Shopping]]\n![](assets/bread.jpg)<!-- {"width":200} -->',
        'Recipes.textbundle/info.json': info({ uniqueIdentifier: 'BEAR-1', pinned: 1 }),
        'Recipes.textbundle/assets/bread.jpg': new Uint8Array([1, 2]),
        'Shopping.textbundle/text.markdown': '# Shopping\nmilk',
        'Shopping.textbundle/info.json': info({
          uniqueIdentifier: 'BEAR-2',
          archived: 1,
          trashed: 1,
        }),
      }),
      bearImporter,
    )
    const recipes = note(p, 'Recipes')
    expect(recipes.key).toBe('BEAR-1')
    expect(recipes.pinned).toBe(true)
    expect(recipes.created).toBe((700000000 + 978307200) * 1000)
    expect(recipes.updated).toBe(Date.UTC(2024, 4, 1, 10))
    expect(recipes.tags).toEqual(['food/baking', 'two words'])
    expect(recipes.body).toContain('Bread underlined')
    expect(recipes.body).toContain('[[Shopping]]')
    expect(recipes.body).toContain('![](attachments/bread.jpg)')
    expect(recipes.body).not.toContain('<!--')
    const shopping = note(p, 'Shopping')
    expect(shopping.trashed).toBe(true)
    expect(shopping.tags).toEqual(['archived'])
    expect(kinds(p.report)).toEqual(['simplified underline'])
  })

  it('tells multi word tags from headings', () => {
    expect(bearTags('#one and #two words# but # heading')).toEqual(['one', 'two words'])
  })
})

describe('notion markdown', () => {
  const id = (n: number) => n.toString(16).padStart(32, 'a')
  it('reads pages with subpages, properties, databases and links', async () => {
    const p = await run(
      memorySource('Export.zip', {
        [`Team ${id(1)}.md`]: `# Team wiki with a very long title\n\nOwner: Sam\nCreated: January 15, 2024 10:30 AM\nTags: wiki, team\n\nSee [Tasks](Team%20${id(1)}/Tasks%20${id(2)}.md) and [db](Team%20${id(1)}/Board%20${id(3)}.csv).`,
        [`Team ${id(1)}/Tasks ${id(2)}.md`]: `# Tasks\n\n![chart](Tasks%20${id(2)}/chart.png)\n[out](https://www.notion.so/x)`,
        [`Team ${id(1)}/Tasks ${id(2)}/chart.png`]: new Uint8Array([3]),
        [`Team ${id(1)}/Board ${id(3)}.csv`]: 'Name,Status\n"Fix, then ship",Done\nPlan,"To do"\n',
        [`Team ${id(1)}/Board ${id(3)}_all.csv`]: 'Name,Status\n',
        [`Team ${id(1)}/Board ${id(3)}/Plan ${id(4)}.md`]: '# Plan\n\nStatus: To do\n\nSteps',
      }),
      notionImporter,
    )
    const team = note(p, 'Team wiki with a very long title')
    expect(team.key).toBe(id(1))
    expect(team.folder).toBe(`Team ${id(1)}`)
    expect(team.tags).toEqual(['wiki', 'team'])
    expect(team.created).toBe(new Date(2024, 0, 15, 10, 30).getTime())
    expect(team.body).toContain('- **Owner:** Sam')
    expect(team.body).toContain('[[Tasks]]')
    expect(team.body).toContain('[[Board|db]]')
    const tasks = note(p, 'Tasks')
    // its folder holds only its picture: no folder of its own
    expect(tasks.folder).toBe(`Team ${id(1)}`)
    expect(tasks.body).toContain('![chart](attachments/chart.png)')
    expect(note(p, 'Board').body).toBe(
      '| Name | Status |\n| --- | --- |\n| Fix, then ship | Done |\n| Plan | To do |',
    )
    expect(note(p, 'Plan').folder).toBe(`Team ${id(1)}/Board ${id(3)}`)
    expect(p.model.folders.map((f) => f.name)).toEqual(['Team', 'Board'])
    expect(kinds(p.report)).toEqual(['simplified database'])
  })

  it('does not make folders of the zips the export is split into', async () => {
    const p = await run(
      memorySource('Export.zip', {
        [`Export-Part-1.zip/Top ${id(5)}.md`]: '# Top',
        [`Export-Part-1.zip/Top ${id(5)}/Child ${id(6)}.md`]: '# Child',
      }),
      notionImporter,
    )
    expect(p.model.folders).toEqual([
      { key: `Export-Part-1.zip/Top ${id(5)}`, parent: null, name: 'Top' },
    ])
    expect(note(p, 'Top').folder).toBe(`Export-Part-1.zip/Top ${id(5)}`)
  })

  it('parses csv and properties strictly', () => {
    expect(parseCsv('a,"b ""c""\nd",e\r\n\n1,2,3')).toEqual([
      ['a', 'b "c"\nd', 'e'],
      ['1', '2', '3'],
    ])
    expect(csvTable([['a|b'], ['x']])).toBe('| a\\|b |\n| --- |\n| x |')
    expect(notionProperties('Just text: with a colon later').body).toBe(
      '- **Just text:** with a colon later\n\n',
    )
    expect(notionProperties('- list: item').tags).toEqual([])
  })
})

describe('simplenote and standard notes', () => {
  it('reads simplenote notes and its trash', async () => {
    const p = await run(
      memorySource('notes.zip', {
        'source/notes.json': JSON.stringify({
          activeNotes: [
            {
              id: 's1',
              content: 'Groceries\r\n\r\n- eggs',
              creationDate: '2021-02-03T04:05:06.000Z',
              lastModified: '2022-01-01T00:00:00.000Z',
              pinned: true,
              tags: ['home'],
            },
          ],
          trashedNotes: [{ id: 's2', content: 'Old', creationDate: '2020-01-01T00:00:00.000Z' }],
        }),
      }),
      simplenoteImporter,
    )
    expect(note(p, 'Groceries')).toMatchObject({
      key: 's1',
      body: '- eggs',
      tags: ['home'],
      pinned: true,
      created: Date.UTC(2021, 1, 3, 4, 5, 6),
    })
    expect(note(p, 'Old').trashed).toBe(true)
  })

  it('reads a standard notes backup with tags, super notes, files and encrypted items', async () => {
    const lexical = {
      root: {
        children: [
          { type: 'heading', tag: 'h2', children: [{ type: 'text', text: 'Plan' }] },
          {
            type: 'paragraph',
            children: [
              { type: 'text', text: 'bold', format: 1 },
              { type: 'text', text: ' and ' },
              { type: 'link', url: 'https://e.org', children: [{ type: 'text', text: 'link' }] },
            ],
          },
          {
            type: 'list',
            listType: 'check',
            children: [
              { type: 'listitem', checked: true, children: [{ type: 'text', text: 'done' }] },
            ],
          },
          { type: 'collapsible-container', children: [{ type: 'text', text: 'inside' }] },
        ],
      },
    }
    const p = await run(
      memorySource('Standard Notes Backup - Jan 1.txt', {
        'Standard Notes Backup - Jan 1.txt': JSON.stringify({
          version: '004',
          items: [
            {
              uuid: 'n1',
              content_type: 'Note',
              created_at: '2023-01-01T00:00:00Z',
              updated_at: '2023-02-01T00:00:00Z',
              content: {
                title: 'Plain',
                text: 'hello',
                appData: { 'org.standardnotes.sn': { pinned: true, archived: true } },
              },
            },
            {
              uuid: 'n2',
              content_type: 'Note',
              created_at: '2023-01-01T00:00:00Z',
              content: {
                title: 'Super',
                noteType: 'super',
                text: JSON.stringify(lexical),
                trashed: true,
              },
            },
            {
              uuid: 't1',
              content_type: 'Tag',
              content: { title: 'work', references: [{ uuid: 'n1', content_type: 'Note' }] },
            },
            { uuid: 'f1', content_type: 'SN|FileItem', content: { name: 'scan.pdf' } },
            { uuid: 'x1', content_type: 'Note', content: '004:sealed' },
            { uuid: 'd1', content_type: 'Note', deleted: true },
          ],
        }),
      }),
      standardNotesImporter,
    )
    expect(note(p, 'Plain')).toMatchObject({ key: 'n1', tags: ['work', 'archived'], pinned: true })
    const sup = note(p, 'Super')
    expect(sup.trashed).toBe(true)
    expect(sup.body).toBe('## Plan\n\n**bold** and [link](https://e.org)\n\n- [x] done\n\ninside')
    expect(kinds(p.report)).toEqual([
      'encrypted x1',
      'fileMissing scan.pdf',
      'simplified collapsible-container',
    ])
    expect(
      lexicalToMarkdown(
        {
          children: [
            {
              type: 'list',
              listType: 'number',
              children: [
                {
                  type: 'listitem',
                  children: [
                    { type: 'text', text: 'a' },
                    {
                      type: 'list',
                      children: [{ type: 'listitem', children: [{ type: 'text', text: 'b' }] }],
                    },
                  ],
                },
              ],
            },
          ],
        },
        new Set(),
      ),
    ).toBe('1. a\n  - b')
  })
})

describe('apple notes and detection', () => {
  it('reads the markdown export and the icloud copy', async () => {
    const md = await run(
      memorySource('Notes', {
        'Recipes/Soup.md': '# Soup\n![](Attachments/pot.jpeg)',
        'Recipes/Attachments/pot.jpeg': new Uint8Array([1]),
      }),
      appleNotesImporter,
    )
    expect(note(md, 'Soup').files.map((f) => f.name)).toEqual(['pot.jpeg'])
    expect(md.model.folders.map((f) => f.name)).toEqual(['Recipes'])
    const cloud = await run(
      memorySource('iCloud', { 'iCloud Notes/Notes/Ideas/Walk.txt': 'go outside' }),
      appleNotesImporter,
    )
    expect(note(cloud, 'Walk').body).toBe('go outside')
  })

  it('falls back to plain markdown', () => {
    expect(detect(memorySource('x', { 'a.md': 'a' }), IMPORTERS)).toBe(markdownImporter)
  })
})

describe('preview snippets', () => {
  it('reads as text', () => {
    expect(
      plainSnippet(
        '# Title\n\n![pic](attachments/a.png){width=3} See [[Seeds#Spring|seeds]], [web](https://e.org) **bold** `code`\n- [x] done',
      ),
    ).toBe('Title See seeds, web bold code done')
    expect(plainSnippet('| Name | Status |\n| --- | :-: |\n| Ship | Done |')).toBe(
      'Name Status Ship Done',
    )
  })
})
