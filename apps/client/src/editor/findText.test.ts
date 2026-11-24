// SPDX-License-Identifier: AGPL-3.0-or-later

import { Editor } from '@tiptap/core'
import { describe, expect, it } from 'vitest'
import { findKey, setFindCurrent, setFindQuery } from './extensions/find'
import { createExtensions } from './extensions'
import { findAll, findInDoc, nearest } from './findText'

function editorWith(markdown: string) {
  return new Editor({
    element: document.createElement('div'),
    extensions: createExtensions(),
    content: markdown,
    contentType: 'markdown',
  })
}

/** The text of each match, as the note shows it. */
const textsIn = (text: string, query: string) =>
  findAll(text, query).map(({ from, to }) => text.slice(from, to))

const textsInNote = (markdown: string, query: string) => {
  const { doc } = editorWith(markdown).state
  return findInDoc(doc, query).map(({ from, to }) => doc.textBetween(from, to, '|'))
}

describe('findAll', () => {
  it('finds whatever the case', () => {
    expect(textsIn('Paris, PARIS and paris', 'pARis')).toEqual(['Paris', 'PARIS', 'paris'])
  })

  it('finds with or without accents, either way round', () => {
    expect(textsIn('Un café à Évian', 'cafe a evian')).toEqual(['café à Évian'])
    expect(textsIn('Un cafe a Evian', 'café À ÉVIAN')).toEqual(['cafe a Evian'])
    expect(textsIn('Straße, Ångström, naïve', 'angstrom')).toEqual(['Ångström'])
  })

  it('takes an accent written as its own mark with its letter', () => {
    const text = 'café noir'
    expect(findAll(text, 'café')).toEqual([{ from: 0, to: 5 }])
    expect(textsIn(text, 'e noir')).toEqual(['é noir'])
  })

  it('finds each place once, one after another', () => {
    expect(findAll('aaaa', 'aa')).toEqual([
      { from: 0, to: 2 },
      { from: 2, to: 4 },
    ])
  })

  it('finds nothing when the words are not there or nothing was typed', () => {
    expect(findAll('Shopping list', 'groceries')).toEqual([])
    expect(findAll('Shopping list', '')).toEqual([])
  })
})

describe('findInDoc', () => {
  it('finds a word that bold, italic or a link cut in the middle', () => {
    expect(textsInNote('A **bo**ld *wo*rd and a [li](https://example.com)nk', 'bold')).toEqual([
      'bold',
    ])
    expect(textsInNote('A **bo**ld *wo*rd and a [li](https://example.com)nk', 'link')).toEqual([
      'link',
    ])
    expect(textsInNote('Un **É**lan', 'elan')).toEqual(['Élan'])
  })

  it('finds in headings, lists, quotes, code and tables', () => {
    const note = [
      '# Plan',
      '',
      '- plan one',
      '',
      '> a plan',
      '',
      '```',
      'plan()',
      '```',
      '',
      '| Plan |',
      '| --- |',
      '| plan |',
    ].join('\n')
    expect(textsInNote(note, 'plan')).toHaveLength(6)
  })

  it('keeps each match within its paragraph and away from line breaks', () => {
    expect(textsInNote('the end\n\nstart over', 'endstart')).toEqual([])
    expect(textsInNote('the end\n\nstart over', 'end start')).toEqual([])
    expect(textsInNote('one\\\ntwo', 'one two')).toEqual([])
  })

  it('finds nothing in a note without the words', () => {
    expect(textsInNote('# Recipes\n\nFlour, eggs, milk', 'sugar')).toEqual([])
  })
})

describe('nearest', () => {
  const found = [
    { from: 2, to: 5 },
    { from: 10, to: 13 },
  ]

  it('picks the first match after a place, going round past the last', () => {
    expect(nearest(found, 0)).toBe(0)
    expect(nearest(found, 6)).toBe(1)
    expect(nearest(found, 20)).toBe(0)
    expect(nearest([], 3)).toBe(-1)
  })
})

describe('FindInNote', () => {
  it('marks every match and the current one, and follows edits', () => {
    const editor = editorWith('red green red blue red')
    editor.view.dispatch(setFindQuery(editor.state.tr, 'RED'))
    const shown = () => findKey.getState(editor.state)
    expect(shown()?.found).toHaveLength(3)
    expect(shown()?.marks.find()).toHaveLength(3)
    editor.view.dispatch(setFindCurrent(editor.state.tr, 2))
    expect(shown()?.current).toBe(2)
    editor.commands.insertContentAt(1, 'red ')
    expect(shown()?.found).toHaveLength(4)
    editor.view.dispatch(setFindQuery(editor.state.tr, ''))
    expect(shown()?.found).toHaveLength(0)
    expect(shown()?.marks.find()).toHaveLength(0)
  })

  it('leaves the saved note as it was', () => {
    const editor = editorWith('red green')
    editor.view.dispatch(setFindQuery(editor.state.tr, 'red'))
    expect(editor.getMarkdown().trim()).toBe('red green')
  })
})
