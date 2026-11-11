// SPDX-License-Identifier: AGPL-3.0-or-later
// A text field whose text is replaced from elsewhere (another device's
// edit) would send its caret to the end. These keep the caret and the
// selection on the same words.

/** Where the place `at` in `old` lies in `next`: a change before it moves
 * it along, a change after it leaves it, a change around it puts it at
 * the end of what changed. */
export function caretAfterChange(old: string, next: string, at: number): number {
  const shorter = Math.min(old.length, next.length)
  let same = 0
  while (same < shorter && old[same] === next[same]) same++
  let tail = 0
  while (tail < shorter - same && old[old.length - 1 - tail] === next[next.length - 1 - tail])
    tail++
  if (at <= same) return at
  if (at >= old.length - tail) return at + next.length - old.length
  return next.length - tail
}

/** Call before `field` takes the text `next`, then call what it returns
 * once the field shows it. Nothing to do, and nothing returned, when the
 * field already shows it (the person's own typing) or has no caret. */
export function keepCaret(
  field: HTMLInputElement | HTMLTextAreaElement,
  next: string,
): (() => void) | undefined {
  const old = field.value
  if (old === next || document.activeElement !== field) return
  const start = caretAfterChange(old, next, field.selectionStart ?? old.length)
  const end = caretAfterChange(old, next, field.selectionEnd ?? old.length)
  return () => field.setSelectionRange(start, end)
}
