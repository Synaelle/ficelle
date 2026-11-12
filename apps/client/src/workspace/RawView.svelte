<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A note's Markdown, edited as text. A text field shows no colours, so a
     copy of the text sits under it: line numbers in the margin and the
     Markdown marks dimmed, while the field above, its own text invisible,
     takes the typing and draws the caret and the selection. Both wrap the
     same way, character for character. -->
<script lang="ts">
  import { tick } from 'svelte'
  import { keepCaret } from '../lib/caret'

  let {
    value,
    label,
    oninput,
    field = $bindable(),
  }: {
    value: string
    label: string
    oninput: (value: string) => void
    field?: HTMLTextAreaElement
  } = $props()

  // text from another device: the caret stays on the same words
  $effect.pre(() => {
    const restore = field ? keepCaret(field, value) : undefined
    if (restore) void tick().then(restore)
  })

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
</script>

<!-- the margin is as wide as the last line's number -->
<div class="raw" style:--digits={Math.max(3, String(lines.length).length)}>
  <div class="copy" aria-hidden="true">
    {#each lines as line, index (index)}
      <div class="line">
        {#each line as part, at (at)}<span class:mark={part.mark}>{part.text}</span>{/each}
      </div>
    {/each}
  </div>
  <textarea
    bind:this={field}
    spellcheck="false"
    aria-label={label}
    {value}
    oninput={(e) => oninput(e.currentTarget.value)}></textarea>
</div>

<style>
  .raw {
    --gutter: calc(var(--digits, 3) * 1ch + 1.4em);
    position: relative;
    counter-reset: line;
    font-family: var(--font-mono);
    font-size: 14px;
    line-height: 1.7;
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
    min-height: 1.7em;
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
