<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A scrolling list that only keeps the rows in view (plus a margin) in the
     page, so thousands of notes load and scroll like a few dozen. Rows must
     all have the same height, it is measured from the first one.
     The list is one Tab stop: the arrows, Home, End and the page keys move
     between rows, so Tab goes on past it to the note. -->
<script lang="ts" generics="T">
  import { tick, type Snippet } from 'svelte'

  let {
    items,
    key,
    row,
    empty,
    label,
    active,
  }: {
    items: T[]
    key: (item: T) => string
    /** The row, and whether it holds the list's Tab stop (tabindex 0, -1 otherwise). */
    row: Snippet<[T, boolean]>
    empty: Snippet
    label: string
    /** The row Tab lands on while the keys have not moved, the open note. */
    active?: string
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

  /** The row last reached with the keys or a click, and where it was. */
  let cursor: string | null = $state(null)
  let cursorAt = 0
  /** The row holding the Tab stop. It must be drawn: when the cursor and the
   * active row are scrolled away, the first row in view takes it. A row gone
   * from the list (to the trash) leaves it to the one now in its place. */
  const stop = $derived.by(() => {
    const drawn = visible.map(key)
    const gone = cursor !== null && !items.some((item) => key(item) === cursor)
    const inPlace = gone ? items[Math.min(cursorAt, items.length - 1)] : undefined
    for (const wanted of [inPlace && key(inPlace), cursor, active])
      if (wanted && drawn.includes(wanted)) return wanted
    const top = items[Math.min(items.length - 1, Math.floor(scrollTop / rowHeight))]
    return top === undefined ? undefined : key(top)
  })

  /** Gives the focus to the row holding the Tab stop. */
  export function focusStop() {
    const at = items.findIndex((item) => key(item) === stop)
    if (at >= 0) void focusRow(at)
  }

  function onkeydown(event: KeyboardEvent) {
    const at = rowIndex(event.target)
    if (at === null || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
    const page = Math.max(1, Math.floor(viewport / rowHeight) - 1)
    const moves: Record<string, number> = {
      ArrowDown: at + 1,
      ArrowUp: at - 1,
      Home: 0,
      End: items.length - 1,
      PageDown: at + page,
      PageUp: at - page,
    }
    const to = moves[event.key]
    if (to === undefined) return
    // the list would scroll instead, taking the focused row out of the page
    event.preventDefault()
    void focusRow(Math.max(0, Math.min(items.length - 1, to)))
  }

  function rowIndex(target: EventTarget | null): number | null {
    const li = (target as Element | null)?.closest('li[data-row]')
    return li ? Number(li.getAttribute('aria-posinset')) - 1 : null
  }

  /** Scrolls a row into view, draws it and gives it the focus. */
  async function focusRow(index: number) {
    const item = items[index]
    if (!list || item === undefined) return
    const top = index * rowHeight
    if (top < list.scrollTop) list.scrollTop = top
    else if (top + rowHeight > list.scrollTop + list.clientHeight)
      list.scrollTop = top + rowHeight - list.clientHeight
    scrollTop = list.scrollTop
    cursor = key(item)
    cursorAt = index
    await tick()
    list.querySelector<HTMLElement>(`li[aria-posinset="${index + 1}"] [tabindex="0"]`)?.focus()
  }

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

<!-- the keys come from the focused row inside, the list takes no focus itself -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<ul
  class="rows"
  aria-label={label}
  bind:this={list}
  onscroll={() => (scrollTop = list?.scrollTop ?? 0)}
  {onkeydown}
  onfocusin={(e) => {
    const at = rowIndex(e.target)
    const item = at === null ? undefined : items[at]
    if (at === null || item === undefined) return
    cursor = key(item)
    cursorAt = at
  }}
>
  {#if items.length === 0}
    {@render empty()}
  {:else}
    <li class="spacer" aria-hidden="true" style:height="{first * rowHeight}px"></li>
    {#each visible as item, i (key(item))}
      <li data-row aria-posinset={first + i + 1} aria-setsize={items.length}>
        {@render row(item, key(item) === stop)}
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
