<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A scrolling list that only keeps the rows in view (plus a margin) in the
     page, so thousands of notes load and scroll like a few dozen. Rows must
     all have the same height, it is measured from the first one. -->
<script lang="ts" generics="T">
  import type { Snippet } from 'svelte'

  let {
    items,
    key,
    row,
    empty,
    label,
  }: {
    items: T[]
    key: (item: T) => string
    row: Snippet<[T]>
    empty: Snippet
    label: string
  } = $props()

  /** Rows drawn above and below the visible ones, so fast flicks stay filled. */
  const OVERSCAN = 8
  /** Until a row is measured, close to the real height. */
  const GUESS = 72

  let list: HTMLUListElement | undefined = $state()
  let rowHeight = $state(GUESS)
  let scrollTop = $state(0)
  let viewport = $state(800)

  const first = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN))
  const last = $derived(
    Math.min(items.length, Math.ceil((scrollTop + viewport) / rowHeight) + OVERSCAN),
  )
  const visible = $derived(items.slice(first, last))

  // the list height changes with the window, row height with fonts and themes
  $effect(() => {
    // rows appear once there are items, measure them then
    if (!list || items.length === 0) return
    const observer = new ResizeObserver(() => {
      if (!list) return
      viewport = list.clientHeight
      const measured = list.querySelector<HTMLElement>('li[data-row]')?.offsetHeight
      if (measured) rowHeight = measured
    })
    observer.observe(list)
    const firstRow = list.querySelector('li[data-row]')
    if (firstRow) observer.observe(firstRow)
    return () => observer.disconnect()
  })

  // a new list (another folder, a search) starts at the top
  let shown: T[] | undefined
  $effect(() => {
    if (items !== shown && list) {
      if (shown !== undefined) list.scrollTop = 0
      shown = items
    }
  })
</script>

<ul
  class="rows"
  aria-label={label}
  bind:this={list}
  onscroll={() => (scrollTop = list?.scrollTop ?? 0)}
>
  {#if items.length === 0}
    {@render empty()}
  {:else}
    <li class="spacer" aria-hidden="true" style:height="{first * rowHeight}px"></li>
    {#each visible as item, i (key(item))}
      <li data-row aria-posinset={first + i + 1} aria-setsize={items.length}>
        {@render row(item)}
      </li>
    {/each}
    <li class="spacer" aria-hidden="true" style:height="{(items.length - last) * rowHeight}px"></li>
  {/if}
</ul>

<style>
  .rows {
    flex: 1;
    margin: 0;
    padding: 0 var(--space-2);
    list-style: none;
    overflow-x: hidden;
    overflow-y: auto;
    /* scroll anchoring would fight the spacers while rows swap */
    overflow-anchor: none;
  }
  .spacer {
    padding: 0;
  }
</style>
