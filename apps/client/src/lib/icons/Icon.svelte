<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- One icon in the set of the current theme. Decorative by default, pass
     `label` when the icon is the only content of a control. -->
<script lang="ts">
  import { lucide, phosphor } from 'virtual:icons'
  import { currentTheme } from '../theme.svelte'
  import type { IconName } from './sets'
  import { stitch } from './stitch'

  let { name, size = 18, label }: { name: IconName; size?: number; label?: string } = $props()

  const set = $derived(currentTheme().icons)
  const inner = $derived(
    set === 'stitch'
      ? (stitch[name] ?? lucide[name])
      : set === 'phosphor-regular'
        ? phosphor[name]
        : lucide[name],
  )
  // phosphor draws on a 256 grid with fills, lucide and stitch on 24 with strokes
  const viewBox = $derived(set === 'phosphor-regular' ? '0 0 256 256' : '0 0 24 24')
  const stroked = $derived(set !== 'phosphor-regular' || inner === undefined)
</script>

<svg
  class="icon"
  class:stroked
  class:stitch={set === 'stitch'}
  width={size}
  height={size}
  {viewBox}
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
>
  <!-- the markup comes from icon files bundled at build time, never from user data -->
  <!-- eslint-disable-next-line svelte/no-at-html-tags -->
  {@html inner ?? ''}
</svg>

<style>
  .icon {
    flex: none;
    display: block;
  }
  .stroked {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.75;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .icon:not(.stroked) {
    fill: currentColor;
  }
  .stitch :global(.st) {
    stroke-dasharray: 2.2 2.6;
  }
  .stitch :global(.dot) {
    fill: currentColor;
    stroke: none;
  }
  .stitch :global(.gap) {
    stroke: var(--icon-gap, var(--color-bg));
    stroke-width: 4.5;
  }
</style>
