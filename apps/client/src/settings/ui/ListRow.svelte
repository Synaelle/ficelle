<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A row of the phone settings, a button as a whole: its icon,
     its name with the current value under it, and on the right a switch, a
     chevron for choices folded under it or an arrow for a page. -->
<script lang="ts">
  import Icon from '../../lib/icons/Icon.svelte'
  import type { IconName } from '../../lib/icons/sets'
  import Switch from './Switch.svelte'

  let {
    icon,
    label,
    value,
    on,
    action = false,
    folds,
    open = false,
    typed = false,
    onclick,
  }: {
    icon: IconName
    label: string
    value?: string
    /** A switch instead of a value. */
    on?: boolean
    /** Does its thing at once, no page opens: no arrow. */
    action?: boolean
    /** Opens its choices in place, under it: the id of their list. */
    folds?: string
    /** With `folds`: whether they show. */
    open?: boolean
    /** The label shows typed characters (->) that no font may join. */
    typed?: boolean
    onclick: (event: MouseEvent) => void
  } = $props()
</script>

<button
  class="row list-row"
  role={on === undefined ? undefined : 'switch'}
  aria-checked={on}
  aria-controls={folds}
  aria-expanded={folds ? open : undefined}
  {onclick}
>
  <span class="tile"><Icon name={icon} size={16} /></span>
  <!-- the value under its label: long words in any language keep room -->
  <span class="text">
    <span class="label" class:as-typed={typed}>{label}</span>
    {#if on === undefined && value}<span class="value">{value}</span>{/if}
  </span>
  {#if on !== undefined}
    <Switch drawn checked={on} />
  {:else if folds}
    <span class="chev" class:open><Icon name="down" size={16} /></span>
  {:else if !action}
    <Icon name="right" size={16} />
  {/if}
</button>

<style>
  .row {
    position: relative;
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 52px;
    padding: 0 var(--space-4);
    border: 0;
    background: none;
    color: var(--color-text);
    font: inherit;
    font-size: 16px;
    text-align: left;
  }
  .row:active {
    background: var(--color-surface-2);
  }
  /* hairlines between rows start after the icon, like a phone's own lists,
     also under choices or a download line folded in between */
  :global(:is(.list-row, .choices, .under) + .list-row)::before {
    content: '';
    position: absolute;
    top: 0;
    left: calc(var(--space-4) + var(--space-3) + 30px);
    right: var(--space-4);
    border-top: 1px solid var(--color-border);
  }
  .row :global(.icon) {
    flex: none;
  }
  .row > :global(.icon:last-child) {
    color: var(--color-text-faint);
  }
  .tile {
    width: 30px;
    height: 30px;
    flex: none;
    display: grid;
    place-items: center;
    /* the icon in the accent: the colour says "a setting", not which kind */
    color: var(--color-accent);
  }
  .tile :global(.icon) {
    color: inherit;
  }
  .chev {
    display: grid;
    flex: none;
    color: var(--color-text-faint);
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .chev.open {
    transform: rotate(180deg);
  }
  @media (prefers-reduced-motion: reduce) {
    .chev {
      transition: none;
    }
  }
  /* with a large text size the value gives way first, cut short, and the
     label wraps if it still has no room: never drawn over each other. A
     label with its value fits the height of a row without one */
  .text {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: var(--space-1) 0;
  }
  .label {
    overflow-wrap: anywhere;
    hyphens: auto;
  }
  .value {
    color: var(--color-text-muted);
    font-size: 14px;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }
</style>
