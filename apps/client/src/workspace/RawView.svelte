<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A note's Markdown, edited as text. A text field shows no colours, so a
     copy of the text sits under it: line numbers in the margin and the
     Markdown marks dimmed, while the field above, its own text invisible,
     takes the typing and draws the caret and the selection. Both wrap the
     same way, character for character. -->
<script lang="ts">
  import { tick } from 'svelte'
  import { keepCaret } from '../lib/caret'
  import { LEADING, prefs, shortcutFor } from '../lib/prefs.svelte'
  import { bindingOf } from '../lib/shortcuts'
  import type { Found } from '../editor/findText'

  let {
    value,
    label,
    oninput,
    field = $bindable(),
    found = [],
    current = -1,
  }: {
    value: string
    label: string
    oninput: (value: string) => void
    field?: HTMLTextAreaElement
    /** Matches of the find bar in `value`, marked in the text. */
    found?: Found[]
    /** The match shown, marked more strongly. */
    current?: number
  } = $props()

  // text from another device: the caret stays on the same words
  $effect.pre(() => {
    const restore = field ? keepCaret(field, value) : undefined
    if (restore) void tick().then(restore)
  })

  /** The delete-line shortcut, as in the formatted view: the caret's line
   * goes, its newline with it. */
  function keydown(event: KeyboardEvent & { currentTarget: HTMLTextAreaElement }) {
    if (bindingOf(event) !== shortcutFor('deleteLine')) return
    event.preventDefault()
    const area = event.currentTarget
    const text = area.value
    const caret = area.selectionStart
    let from = text.lastIndexOf('\n', caret - 1) + 1
    let to = text.indexOf('\n', caret)
    if (to === -1) [from, to] = [Math.max(0, from - 1), text.length]
    else to += 1
    area.setRangeText('', from, to, 'start')
    oninput(area.value)
  }

  /** The Markdown marks of a line, dimmed: what starts a heading, a list
   * item, a task or a quote, a fence, and the marks around bold, struck
   * through, code and links between notes. */
  const BLOCK = /^(\s*)(#{1,6} |>+ ?|```.*|~~~.*|(?:[-*+]|\d{1,9}[.)]) (?:\[[ xX]\] )?)/
  const INLINE = /\*\*|__|~~|`+|\[\[|\]\]/g

  function parts(line: string) {
    const out: { text: string; mark: boolean }[] = []
    let rest = line
    const block = BLOCK.exec(line)
    if (block) {
      out.push({ text: block[1] ?? '', mark: false }, { text: block[2] ?? '', mark: true })
      rest = line.slice(block[0].length)
      // a fence line is all mark
      if (/^\s*(```|~~~)/.test(line)) return out
    }
    let at = 0
    for (const found of rest.matchAll(INLINE)) {
      out.push({ text: rest.slice(at, found.index), mark: false })
      out.push({ text: found[0], mark: true })
      at = found.index + found[0].length
    }
    out.push({ text: rest.slice(at), mark: false })
    return out
  }

  // inside a code block every line is code: nothing in it is a mark
  const lines = $derived.by(() => {
    let fence: string | null = null
    return value.split('\n').map((line) => {
      const marker = /^\s*(`{3,}|~{3,})/.exec(line)?.[1]
      if (fence && marker?.startsWith(fence)) fence = null
      else if (fence) return [{ text: line, mark: false }]
      else if (marker) fence = marker
      return parts(line)
    })
  })

  /** The parts again, each cut where a match of the find starts or ends:
   * `match` is the match's number, -1 outside one. */
  const shown = $derived.by(() => {
    let at = 0
    let first = 0
    return lines.map((line) => {
      const out = line.map((part) => {
        const start = at
        const end = (at += part.text.length)
        while (first < found.length && (found[first]?.to ?? 0) <= start) first++
        const pieces: { text: string; match: number }[] = []
        let from = start
        for (let index = first; index < found.length; index++) {
          const match = found[index]
          if (!match || match.from >= end) break
          const begin = Math.max(match.from, start)
          if (begin > from)
            pieces.push({ text: part.text.slice(from - start, begin - start), match: -1 })
          const stop = Math.min(match.to, end)
          pieces.push({ text: part.text.slice(begin - start, stop - start), match: index })
          from = stop
        }
        if (from < end || pieces.length === 0)
          pieces.push({ text: part.text.slice(from - start), match: -1 })
        return { mark: part.mark, pieces }
      })
      // the line's newline
      at += 1
      return out
    })
  })
</script>

<!-- the margin is as wide as the last line's number -->
<div
  class="raw"
  style:--digits={Math.max(3, String(lines.length).length)}
  style:--leading={LEADING[prefs.lineSpacing]}
>
  <div class="copy" aria-hidden="true">
    {#each shown as line, index (index)}
      <div class="line">
        {#each line as part, at (at)}<span class:mark={part.mark}
            >{#each part.pieces as piece, p (p)}{#if piece.match < 0}{piece.text}{:else}<mark
                  class="find-match"
                  class:find-current={piece.match === current}>{piece.text}</mark
                >{/if}{/each}</span
          >{/each}
      </div>
    {/each}
  </div>
  <textarea
    bind:this={field}
    spellcheck="false"
    aria-label={label}
    {value}
    oninput={(e) => oninput(e.currentTarget.value)}
    onkeydown={keydown}></textarea>
</div>

<style>
  .raw {
    --gutter: calc(var(--digits, 3) * 1ch + 1.4em);
    position: relative;
    counter-reset: line;
    font-family: var(--font-mono);
    font-size: 14px;
    /* the line spacing chosen in the settings */
    line-height: var(--leading);
  }
  .copy,
  textarea {
    box-sizing: border-box;
    width: 100%;
    margin: 0;
    padding: 0 0 var(--space-6) var(--gutter);
    border: 0;
    font: inherit;
    letter-spacing: normal;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    tab-size: 4;
  }
  .copy {
    min-height: 40vh;
    color: var(--color-text);
  }
  .line {
    position: relative;
    min-height: calc(var(--leading) * 1em);
    counter-increment: line;
  }
  .line::before {
    content: counter(line);
    position: absolute;
    left: calc(-1 * var(--gutter));
    width: calc(var(--gutter) - 1em);
    text-align: right;
    color: var(--color-text-faint);
    user-select: none;
  }
  .mark {
    color: var(--color-text-muted);
  }
  textarea {
    position: absolute;
    inset: 0;
    height: 100%;
    overflow: hidden;
    resize: none;
    background: transparent;
    color: transparent;
    caret-color: var(--color-text);
    outline: none;
  }
</style>
