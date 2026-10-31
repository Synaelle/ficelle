<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Drag handle that resizes the pane on its left. Arrow keys move it too,
     double click puts the default width back. -->
<script lang="ts">
  let {
    width = $bindable(),
    min,
    max,
    initial,
    label,
  }: { width: number; min: number; max: number; initial: number; label: string } = $props()

  const STEP = 16
  const clamp = (value: number) => Math.round(Math.min(max, Math.max(min, value)))

  let start: { x: number; width: number } | null = $state(null)

  function down(event: PointerEvent) {
    if (event.button !== 0) return
    event.preventDefault()
    ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
    start = { x: event.clientX, width }
  }

  function move(event: PointerEvent) {
    if (start) width = clamp(start.width + event.clientX - start.x)
  }

  function keys(event: KeyboardEvent) {
    const delta = { ArrowLeft: -STEP, ArrowRight: STEP }[event.key]
    if (delta === undefined) return
    event.preventDefault()
    width = clamp(width + delta)
  }
</script>

<!-- a focusable separator with a value is the ARIA window splitter pattern, an
     interactive control the a11y lint does not know -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="splitter"
  class:dragging={start !== null}
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={width}
  aria-valuemin={min}
  aria-valuemax={max}
  tabindex="0"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={() => (start = null)}
  onpointercancel={() => (start = null)}
  ondblclick={() => (width = clamp(initial))}
  onkeydown={keys}
></div>

<style>
  .splitter {
    position: relative;
    z-index: 5;
    width: 8px;
    margin: 0 -4px;
    flex: none;
    cursor: col-resize;
    touch-action: none;
  }
  .splitter::after {
    content: '';
    position: absolute;
    inset: 0 3px;
    transition: background var(--dur-fast) var(--ease-out);
  }
  .splitter:focus-visible::after,
  .dragging::after {
    background: var(--color-accent);
  }
  @media (hover: hover) {
    .splitter:hover::after {
      background: var(--color-accent);
    }
  }
  .splitter:focus-visible {
    outline: none;
  }
</style>
