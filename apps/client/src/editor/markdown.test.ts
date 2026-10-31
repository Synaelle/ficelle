// SPDX-License-Identifier: AGPL-3.0-or-later
// Markdown round trip through the real editor schema. Every probe is one
// feature that must survive load and save. Run on every TipTap upgrade.

import { Editor } from '@tiptap/core'
import { TaskList } from '@tiptap/extension-list'
import { describe, expect, it } from 'vitest'
import { createExtensions } from './extensions'
import { TASK_GUARD, taskListExtent, withBoundedStart } from './extensions/markdownFixes'
import corpus from './fixtures/corpus.md?raw'

function roundTrip(markdown: string): string {
  const editor = new Editor({
    element: document.createElement('div'),
    extensions: createExtensions(),
    content: markdown,
    contentType: 'markdown',
  })
  const out = editor.getMarkdown()
  editor.destroy()
  return out
}

const PROBES: [string, RegExp][] = [
  ['atx headings h1 to h6', /^###### Heading six$/m],
  ['strike', /~~strike~~/],
  ['escaped star', /\\\* not emphasis \\\*/],
  ['escaped brackets', /\\\[not a link\\\]/],
  ['escaped backtick', /\\`not code\\`/],
  ['literal backslash', /a\\\\b backslash/],
  ['french accents', /« Élève à l'école », ça, où, garçon, cœur, naïve, Noël\./],
  ['link title', /\[link with title\]\(https:\/\/example\.com "Title"\)/],
  ['autolink', /<https:\/\/example\.org>|\[https:\/\/example\.org\]\(https:\/\/example\.org\)/],
  ['image', /!\[An image\]\(attachments\/photo\.png\)/],
  ['image layout block', /!\[Alt text\]\(attachments\/img\.png\)\{width=320 wrap=left\}/],
  ['hard break', /hard break( {2}|\\)\nnext line after spaces/],
  ['backslash hard break', /backslash hard break( {2}|\\)\nnext line after backslash/],
  ['nested bullets', /\n {2,4}[-*+] Nested bullet\n {4,8}[-*+] Deeply nested/],
  ['ordered start 7', /^7\. Starts at seven/m],
  ['nested ordered', /\n {3,4}1\. Nested ordered/],
  ['code block in list item', /Item with code block:\n\n? {2,4}```js\n {2,4}const x = 1;/],
  ['line start # kept escaped', /^\\# Not a heading/m],
  ['line start 1. kept escaped', /^1\\\. Not a list/m],
  ['line start - kept escaped', /^\\- Not a bullet/m],
  ['entity', /a (&amp;|&) entity/],
  ['nested blockquote', /> > Nested quote\./],
  ['task in blockquote', /> [-*+] \[ \] Task inside a blockquote\n> [-*+] \[x\] Done task/],
  ['task states', /[-*+] \[ \] Open task\n[-*+] \[x\] Done task/],
  ['nested task', /\n {2,4}[-*+] \[ \] Nested open task/],
  ['fence with language', /```ts\nfunction hello/],
  ['fence without language', /```\nno language fence\n```/],
  ['horizontal rule', /^(---|\*\*\*|___)$/m],
  ['table alignment', /\|\s*:-+\s*\|\s*:-+:\s*\|\s*-+:\s*\|\s*-+\s*\|/],
  ['escaped pipe in cell', /pipe \\\| escaped/],
  ['wiki link in table', /\| *\[\[Note\]\] *\|/],
  ['wiki alias', /\[\[Note\|alias\]\]/],
  ['wiki heading alias', /\[\[Note#Heading\|alias\]\]/],
  ['inline math', /\$E = mc\^2\$/],
  ['block math', /\$\$\n?\\int_0\^1 x\^2 \\, dx = \\frac\{1\}\{3\}\n?\$\$/],
  ['callout with title', /> \[!note\] Callout title\n> Callout body line one\./],
  ['callout without title', /> \[!warning\]\n> Warning without title\./],
  ['emphasis next to punctuation', /\(([*_])paren\1\)/],
  [
    'accented urls verbatim',
    /\[lien accentué\]\(notes\/Été\.md\) and !\[été\]\(attachments\/été\.png\)/,
  ],
  ['soft line breaks', /A blockquote line\.\n> Second line/],
  ['intraword bold', /intra\*\*word\*\*bold/],
]

/** Losses accepted for now. */
const KNOWN_LOSSES: [string, RegExp][] = [
  ['inline html', /<span>inline html<\/span>/],
  ['loose list stays loose', /Item with code block:\n\n {2,4}```js/],
]

describe('markdown round trip', () => {
  const out = roundTrip(corpus)

  it.each(PROBES)('keeps %s', (_name, probe) => {
    expect(out).toMatch(probe)
  })

  it('still has only the known losses', () => {
    // when one of these starts passing, move it to PROBES
    for (const [name, probe] of KNOWN_LOSSES) expect(probe.test(out), name).toBe(false)
  })

  it('is stable when saved again', () => {
    expect(roundTrip(out)).toBe(out)
  })

  it('writes no &nbsp; for empty lines, and drops the ones notes already have', () => {
    // two Enter presses leave empty paragraphs, upstream wrote them as &nbsp;
    const editor = new Editor({
      element: document.createElement('div'),
      extensions: createExtensions(),
      content: '<p>first</p><p></p><p></p><p></p><p>second</p>',
    })
    const written = editor.getMarkdown()
    editor.destroy()
    expect(written).not.toMatch(/&nbsp;|\u00a0/)
    expect(written).toMatch(/^first\n\n+second\n?$/)
    // lines written by 0.1.0 come back empty and are not written again
    const cleaned = roundTrip('first\n\n&nbsp;\n\n&nbsp;\n\n\u00a0\n\nsecond\n')
    expect(cleaned).not.toMatch(/&nbsp;|\u00a0/)
    expect(cleaned).toMatch(/^first\n\n+second\n?$/)
    // a non-breaking space inside text is the writer's own and stays
    expect(roundTrip('10\u00a0km away\n')).toContain('10\u00a0km away')
  })
})

const SPEED_BLOCK = `## Section\n\nSome **bold** text with a [[Link]] and \`code\`.\n\n- [ ] task\n- [x] done\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n`

/** Parse time in ms, no editor view: what a note costs before it is drawn. */
function parseTime(markdown: string): number {
  const editor = new Editor({ element: null, extensions: createExtensions() })
  const started = performance.now()
  editor.markdown?.parse(markdown)
  const ms = performance.now() - started
  editor.destroy()
  return ms
}

/** The editor's schema with TaskList's tokenizer as upstream ships it. */
function upstreamTaskJson(markdown: string): unknown {
  const extensions = createExtensions().map((e) =>
    e.name === 'taskList' ? withBoundedStart(TaskList, TASK_GUARD) : e,
  )
  const editor = new Editor({ element: null, extensions })
  const json = editor.markdown?.parse(markdown)
  editor.destroy()
  return json
}

function taskJson(markdown: string): unknown {
  const editor = new Editor({ element: null, extensions: createExtensions() })
  const json = editor.markdown?.parse(markdown)
  editor.destroy()
  return json
}

describe('parsing speed', () => {
  it('grows with the note, never with its square', () => {
    parseTime(SPEED_BLOCK)
    const small = parseTime(SPEED_BLOCK.repeat(1250))
    const large = parseTime(SPEED_BLOCK.repeat(5000))
    // four times the text: quadratic parsing took sixteen times as long
    expect(large / Math.max(small, 1)).toBeLessThan(8)
    expect(large).toBeLessThan(1500)
  })

  it.each([
    ['many task lists', '- [ ] task\n- [x] done\n\n'.repeat(5000)],
    ['one long task list', '- [ ] task\n'.repeat(20_000)],
    ['tasks with indented text and blank lines', '- [ ] task\n\n  more about it\n\n'.repeat(5000)],
  ])('%s stay linear', (_, markdown) => {
    expect(parseTime(markdown)).toBeLessThan(1500)
  })
})

describe('task lists read only their own lines', () => {
  it.each([
    ['two lists apart', '- [ ] a\n- [x] b\n\n- [ ] c\n'],
    ['nested items', '- [ ] a\n  - [x] nested\n    - [ ] deeper\n- [ ] b\n'],
    ['indented text after a blank line', '- [ ] a\n\n  more about a\n\n- [ ] b\n\nafter\n'],
    ['followed by a paragraph', '- [ ] a\n- [ ] b\ntext right after\n\n# Heading\n'],
    ['followed by a bullet list', '- [ ] a\n- plain bullet\n'],
    ['indented items', '   - [ ] three spaces\n   - [x] again\n\nend\n'],
    ['at the end, no newline', 'start\n\n- [ ] last'],
    ['blank lines at the end', '- [ ] a\n\n\n\n'],
    ['code inside an item', '- [ ] a\n\n  ```js\n  const x = 1\n  ```\n\n- [ ] b\n'],
  ])('%s parse as upstream does', (_, markdown) => {
    expect(taskJson(markdown)).toEqual(upstreamTaskJson(markdown))
  })

  it('stops where the list stops', () => {
    expect(taskListExtent('- [ ] a\n- [ ] b\n\n- [ ] c\n')).toBe('- [ ] a\n- [ ] b\n\n')
    expect(taskListExtent('- [ ] a\n\n  more\ntext\n')).toBe('- [ ] a\n\n  more\n')
    expect(taskListExtent('- [ ] a')).toBe('- [ ] a')
  })
})

describe('custom syntax', () => {
  it('keeps prices out of math', () => {
    expect(roundTrip('Costs $5 and $10 today.\n')).toBe('Costs $5 and $10 today.')
  })

  it('writes image height only when the ratio was unlocked', () => {
    const md = '![a](attachments/a.png){width=200 height=90 wrap=right}'
    expect(roundTrip(md)).toBe(md)
    expect(roundTrip('![a](attachments/a.png){wrap=center}')).toBe(
      '![a](attachments/a.png){wrap=center}',
    )
  })
})

describe('plain characters', () => {
  it.each(['a -> b', 'x < y & z > w', 'R&D, 5 > 3'])('keeps %s as typed', (md) => {
    expect(roundTrip(md)).toBe(md)
  })

  it('still escapes text that would read as a tag or an entity', () => {
    const md = 'a \\<tag> and \\&amp;'
    expect(roundTrip(md)).toBe(md)
  })
})

describe('typing lists', () => {
  function typed(text: string, autoLists: boolean): string {
    const editor = new Editor({
      element: document.createElement('div'),
      extensions: createExtensions({ autoLists }),
    })
    // input rules only run for text typed by the user, not inserted content
    for (const char of text) {
      const { from, to } = editor.state.selection
      const handled = editor.view.someProp('handleTextInput', (f) =>
        f(editor.view, from, to, char, () => editor.state.tr.insertText(char, from, to)),
      )
      if (!handled) editor.view.dispatch(editor.state.tr.insertText(char, from, to))
    }
    const out = editor.getMarkdown().trimEnd()
    editor.destroy()
    return out
  }

  it('starts lists by default', () => {
    expect(typed('- one', true)).toBe('- one')
    expect(typed('1. one', true)).toBe('1. one')
  })

  it('keeps the dash as text when turned off', () => {
    expect(typed('- one', false)).toBe('\\- one')
    expect(typed('1. one', false)).toBe('1\\. one')
  })
})

describe('github markdown additions', () => {
  it('keeps footnotes where they are written', () => {
    const out = roundTrip(
      'A claim[^1] and another[^note].\n\n[^1]: The source.\n\n[^note]: More *here*.\n',
    )
    expect(out).toContain('A claim[^1] and another[^note].')
    expect(out).toContain('[^1]: The source.')
    expect(out).toContain('[^note]: More *here*.')
  })

  it('keeps a details section with its title and blocks', () => {
    const md =
      '<details>\n<summary>Spoiler **here**</summary>\n\nHidden text.\n\n- a list\n\n</details>\n'
    const out = roundTrip(md)
    expect(out).toContain('<details>\n<summary>Spoiler **here**</summary>')
    expect(out).toContain('Hidden text.')
    expect(out).toMatch(/[-*+] a list/)
    expect(out).toContain('</details>')
    expect(roundTrip(out)).toBe(out)
  })

  it('reads known emoji shortcodes and leaves the others', () => {
    const out = roundTrip(
      'Done :tada: and :+1:, not :notanemoji: or 10:30:45.\n\n`:smile:` in code.\n',
    )
    expect(out).toContain('Done 🎉 and 👍, not :notanemoji: or 10:30:45.')
    expect(out).toContain('`:smile:` in code.')
  })
})

describe('typing GitHub extras', () => {
  function typedWith(text: string, setup: Parameters<typeof createExtensions>[0]): string {
    const editor = new Editor({
      element: document.createElement('div'),
      extensions: createExtensions(setup),
    })
    for (const char of text) {
      if (char === '\n') {
        editor.commands.splitBlock()
        continue
      }
      const { from, to } = editor.state.selection
      const handled = editor.view.someProp('handleTextInput', (f) =>
        f(editor.view, from, to, char, () => editor.state.tr.insertText(char, from, to)),
      )
      if (!handled) editor.view.dispatch(editor.state.tr.insertText(char, from, to))
    }
    const out = editor.getMarkdown().trimEnd()
    editor.destroy()
    return out
  }

  it('makes a footnote mark and its note as they are typed', () => {
    expect(typedWith('Text[^1] more\n[^1]: The note', {})).toBe('Text[^1] more\n\n[^1]: The note')
  })

  it('turns shortcodes into emoji unless turned off', () => {
    expect(typedWith('Yes :tada:', {})).toBe('Yes 🎉')
    expect(typedWith('Yes :tada:', { emojiShortcodes: false })).toBe('Yes :tada:')
  })

  it('leaves shortcodes in a note as written when turned off', () => {
    const editor = new Editor({
      element: document.createElement('div'),
      extensions: createExtensions({ emojiShortcodes: false }),
      content: 'Done :tada:\n',
      contentType: 'markdown',
    })
    expect(editor.getMarkdown().trimEnd()).toBe('Done :tada:')
    editor.destroy()
  })
})
