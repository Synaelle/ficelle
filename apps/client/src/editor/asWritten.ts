// SPDX-License-Identifier: AGPL-3.0-or-later
// The editor writes Markdown in one form of its own. A note from elsewhere
// (imported, typed as raw text) is written otherwise, and saving the
// editor's form whole would rewrite all of it at the first key: an edit the
// person never made, which every device with the note open makes again,
// and two such rewrites merge into doubled marks. Here the lines the person
// left alone stay as the note has them.

/** Cells of the table two stretches of lines are lined up in, at most.
 * Longer ones are first cut at the lines each has only once. */
const TABLE = 1_000_000

/**
 * Lines `from[a..b)` up with `to[c..d)`: for each line of `from`, the line
 * of `to` it is (the longest run of equal lines in order) goes in `at`.
 */
function lineUp(
  from: string[],
  to: string[],
  at: Int32Array,
  a: number,
  b: number,
  c: number,
  d: number,
) {
  while (a < b && c < d && from[a] === to[c]) at[a++] = c++
  while (a < b && c < d && from[b - 1] === to[d - 1]) at[--b] = --d
  const rows = b - a
  const columns = d - c
  if (rows === 0 || columns === 0) return
  if (rows * columns > TABLE) {
    // the lines each side has once, and both have, in the same order: the
    // stretches between them are lined up on their own
    const once = (lines: string[], lo: number, hi: number) => {
      const seen = new Map<string, number>()
      for (let i = lo; i < hi; i++)
        seen.set(lines[i] as string, seen.has(lines[i] as string) ? -1 : i)
      return seen
    }
    // with no such line (every block written otherwise, only the empty
    // lines alike), each line is taken with how many times it came before:
    // the third empty line with the third
    const counted = (lines: string[], lo: number, hi: number) => {
      const times = new Map<string, number>()
      const seen = new Map<string, number>()
      for (let i = lo; i < hi; i++) {
        const line = lines[i] as string
        const nth = times.get(line) ?? 0
        times.set(line, nth + 1)
        seen.set(`${nth}\n${line}`, i)
      }
      return seen
    }
    const both = (inFrom: Map<string, number>, inTo: Map<string, number>) => {
      const list: [number, number][] = []
      for (const [line, i] of inFrom) {
        const j = inTo.get(line) ?? -1
        if (i >= 0 && j >= 0) list.push([i, j])
      }
      return list
    }
    let pairs = both(once(from, a, b), once(to, c, d))
    if (pairs.length === 0) pairs = both(counted(from, a, b), counted(to, c, d))
    // the longest run of them that rises on both sides
    const last: number[] = []
    const before = new Int32Array(pairs.length).fill(-1)
    pairs.forEach(([, j], p) => {
      let lo = 0
      let hi = last.length
      while (lo < hi) {
        const mid = (lo + hi) >> 1
        if ((pairs[last[mid] as number] as [number, number])[1] < j) lo = mid + 1
        else hi = mid
      }
      if (lo > 0) before[p] = last[lo - 1] as number
      last[lo] = p
    })
    const run: [number, number][] = []
    for (let p = last.at(-1) ?? -1; p >= 0; p = before[p] as number)
      run.push(pairs[p] as [number, number])
    if (run.length === 0) return
    run.reverse()
    let i = a
    let j = c
    for (const [x, y] of run) {
      lineUp(from, to, at, i, x, j, y)
      at[x] = y
      i = x + 1
      j = y + 1
    }
    lineUp(from, to, at, i, b, j, d)
    return
  }
  // longest common run of lines from each place to the end
  const width = columns + 1
  const run = new Uint32Array((rows + 1) * width)
  for (let r = rows - 1; r >= 0; r--)
    for (let k = columns - 1; k >= 0; k--)
      run[r * width + k] =
        from[a + r] === to[c + k]
          ? (run[(r + 1) * width + k + 1] as number) + 1
          : Math.max(run[(r + 1) * width + k] as number, run[r * width + k + 1] as number)
  let r = 0
  let k = 0
  while (r < rows && k < columns) {
    if (from[a + r] === to[c + k]) at[a + r++] = c + k++
    else if ((run[(r + 1) * width + k] as number) >= (run[r * width + k + 1] as number)) r++
    else k++
  }
}

/** For each line of `from`, the line of `to` it is, or -1. */
function linedUp(from: string[], to: string[]) {
  const at = new Int32Array(from.length).fill(-1)
  lineUp(from, to, at, 0, from.length, 0, to.length)
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
 * form, every other line stays as stored, with the line endings the note
 * has (a file from Windows). The caller checks that the text still reads
 * the same.
 */
export function asWritten(stored: string, before: string, now: string): string {
  if (stored === before || before === now) return before === now ? stored : now
  const base = before.split('\n')
  // lines are told apart without the \r a Windows file ends them with,
  // and the editor's lines take one when the note has them
  const raw = stored.split('\n')
  const windows = raw.some((line) => line.endsWith('\r'))
  const kept = windows ? raw.map((line) => line.replace(/\r$/, '')) : raw
  const edited = now.split('\n')
  const inKept = linedUp(base, kept)
  const inEdited = linedUp(base, edited)
  const out: string[] = []
  /** The stored lines from `from`, as written. */
  const asStored = (from: number, count: number) => raw.slice(from, from + count)
  const asTyped = (lines: string[]) => (windows ? lines.map((line) => `${line}\r`) : lines)
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
    if (same(was, typed)) out.push(...asStored(k, here.length))
    else {
      const more = here.length - was.length
      if (more > 0 && same(here.slice(more), was)) out.push(...asStored(k, more), ...asTyped(typed))
      else if (more > 0 && same(here.slice(0, was.length), was))
        out.push(...asTyped(typed), ...asStored(k + was.length, more))
      else {
        // the empty lines around a changed block, when the person left
        // them as they were, stay as many as stored
        const [above, under] = [0, 1].map((end) => empty(was, end) === empty(typed, end))
        out.push(
          ...(above ? asStored(k, empty(here, 0)) : []),
          ...asTyped(
            typed.slice(above ? empty(typed, 0) : 0, typed.length - (under ? empty(typed, 1) : 0)),
          ),
          ...(under ? asStored(keptTo - empty(here, 1), empty(here, 1)) : []),
        )
      }
    }
    if (next >= base.length) break
    out.push(raw[keptTo] as string)
    b = next + 1
    k = keptTo + 1
    e = editedTo + 1
  }
  const text = out.join('\n')
  // the last line of a file has no line ending to carry
  return windows && !stored.endsWith('\r') && text.endsWith('\r') ? text.slice(0, -1) : text
}
