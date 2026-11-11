// SPDX-License-Identifier: AGPL-3.0-or-later
// The editor writes Markdown in one form of its own. A note from elsewhere
// (imported, typed as raw text) is written otherwise, and saving the
// editor's form whole would rewrite all of it at the first key: an edit the
// person never made, which every device with the note open makes again,
// and two such rewrites merge into doubled marks. Here the lines the person
// left alone stay as the note has them.

/** Cells of the table two texts are lined up in, at most. */
const TOO_LARGE = 4_000_000

/** For each line of `from`, the line of `to` it is (the longest run of
 * equal lines in order), or -1. Null when the texts are too large. */
function lineUp(from: string[], to: string[]): Int32Array | null {
  const at = new Int32Array(from.length).fill(-1)
  let head = 0
  while (head < from.length && head < to.length && from[head] === to[head]) {
    at[head] = head
    head++
  }
  let tail = 0
  while (
    tail < from.length - head &&
    tail < to.length - head &&
    from[from.length - 1 - tail] === to[to.length - 1 - tail]
  ) {
    at[from.length - 1 - tail] = to.length - 1 - tail
    tail++
  }
  const rows = from.length - head - tail
  const columns = to.length - head - tail
  if (rows === 0 || columns === 0) return at
  if (rows * columns > TOO_LARGE) return null
  // longest common run of lines from each place to the end
  const width = columns + 1
  const run = new Uint32Array((rows + 1) * width)
  for (let r = rows - 1; r >= 0; r--)
    for (let c = columns - 1; c >= 0; c--)
      run[r * width + c] =
        from[head + r] === to[head + c]
          ? (run[(r + 1) * width + c + 1] as number) + 1
          : Math.max(run[(r + 1) * width + c] as number, run[r * width + c + 1] as number)
  let r = 0
  let c = 0
  while (r < rows && c < columns) {
    if (from[head + r] === to[head + c]) at[head + r++] = head + c++
    else if ((run[(r + 1) * width + c] as number) >= (run[r * width + c + 1] as number)) r++
    else c++
  }
  return at
}

/** How many empty lines `lines` starts with, or ends with when `end`. A
 * block of empty lines only counts as starting with them. */
function empty(lines: string[], end: number) {
  let count = 0
  while (count < lines.length && lines[end ? lines.length - 1 - count : count] === '') count++
  return count === lines.length && end ? 0 : count
}

const same = (a: string[], b: string[]) => a.length === b.length && a.every((l, i) => l === b[i])

/**
 * The text to save. `stored` is the note as it is kept, `before` the
 * editor's own Markdown for that same text, `now` the editor's Markdown
 * with what the person changed. A block the person changed (the lines
 * between two lines left alone, a list or a table whole) takes the editor's
 * form, every other line stays as stored. The caller checks that the text
 * still reads the same.
 */
export function asWritten(stored: string, before: string, now: string): string {
  if (stored === before || before === now) return before === now ? stored : now
  const base = before.split('\n')
  const kept = stored.split('\n')
  const edited = now.split('\n')
  const inKept = lineUp(base, kept)
  const inEdited = lineUp(base, edited)
  if (!inKept || !inEdited) return now
  const out: string[] = []
  let b = 0
  let k = 0
  let e = 0
  for (;;) {
    // the next line all three have
    let next = b
    while (next < base.length && (inKept[next] === -1 || inEdited[next] === -1)) next++
    const keptTo = next < base.length ? (inKept[next] as number) : kept.length
    const editedTo = next < base.length ? (inEdited[next] as number) : edited.length
    const was = base.slice(b, next)
    const here = kept.slice(k, keptTo)
    const typed = edited.slice(e, editedTo)
    // left alone by the person: as stored. Else the editor's lines, with
    // the lines stored around them and not in the editor's form (empty
    // lines, mostly)
    if (same(was, typed)) out.push(...here)
    else {
      const more = here.length - was.length
      if (more > 0 && same(here.slice(more), was)) out.push(...here.slice(0, more), ...typed)
      else if (more > 0 && same(here.slice(0, was.length), was))
        out.push(...typed, ...here.slice(was.length))
      else {
        // the empty lines around a changed block, when the person left
        // them as they were, stay as many as stored
        const [above, under] = [0, 1].map((end) => empty(was, end) === empty(typed, end))
        out.push(
          ...(above ? here.slice(0, empty(here, 0)) : []),
          ...typed.slice(above ? empty(typed, 0) : 0, typed.length - (under ? empty(typed, 1) : 0)),
          ...(under ? here.slice(here.length - empty(here, 1)) : []),
        )
      }
    }
    if (next >= base.length) break
    out.push(kept[keptTo] as string)
    b = next + 1
    k = keptTo + 1
    e = editedTo + 1
  }
  return out.join('\n')
}
