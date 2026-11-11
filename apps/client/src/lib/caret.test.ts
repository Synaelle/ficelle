// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { caretAfterChange, keepCaret } from './caret'

describe('the caret when a text changes from elsewhere', () => {
  const old = 'buy milk and bread'
  it('stays when the change is after it', () => {
    expect(caretAfterChange(old, 'buy milk and bread, eggs', 8)).toBe(8)
  })
  it('moves along when the change is before it', () => {
    expect(caretAfterChange(old, 'today: buy milk and bread', 8)).toBe(15)
    expect(caretAfterChange(old, 'milk and bread', 8)).toBe(4)
  })
  it('stays before text put in right where it is', () => {
    expect(caretAfterChange(old, 'buy milk, eggs and bread', 8)).toBe(8)
  })
  it('goes to the end of a change made around it', () => {
    expect(caretAfterChange(old, 'buy oat drink and bread', 6)).toBe(12)
  })
  it('holds at both ends and in repeated text', () => {
    expect(caretAfterChange(old, `${old}!`, old.length)).toBe(old.length)
    expect(caretAfterChange(old, `> ${old}`, 0)).toBe(0)
    expect(caretAfterChange(old, `> ${old}`, old.length)).toBe(old.length + 2)
    expect(caretAfterChange('aaa', 'aaaa', 1)).toBe(1)
    expect(caretAfterChange('', 'new', 0)).toBe(0)
    expect(caretAfterChange('gone', '', 2)).toBe(0)
  })

  it('a focused field keeps its selection on the same words', () => {
    const field = document.createElement('input')
    document.body.append(field)
    field.value = old
    field.focus()
    field.setSelectionRange(4, 8)
    const next = 'today: buy milk and bread'
    const restore = keepCaret(field, next)
    field.value = next
    restore?.()
    expect(next.slice(field.selectionStart ?? 0, field.selectionEnd ?? 0)).toBe('milk')
    // the person's own typing: the field already shows the text
    expect(keepCaret(field, next)).toBeUndefined()
    field.blur()
    expect(keepCaret(field, 'other')).toBeUndefined()
    field.remove()
  })
})
