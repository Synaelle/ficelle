<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A group of settings rows: an optional title above, a short note below,
     the rows in a rounded panel on the desktop and flat on the page on a
     phone (PhoneSettings). -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    title,
    note,
    typed = false,
    children,
  }: {
    title?: string
    note?: string
    /** The title shows typed characters (->) that no font may join. */
    typed?: boolean
    children: Snippet
  } = $props()
</script>

<section class="group">
  {#if title}<h3 class:as-typed={typed}>{title}</h3>{/if}
  <div class="panel">{@render children()}</div>
  {#if note}<p class="note">{note}</p>{/if}
</section>

<style>
  .group {
    display: grid;
    gap: var(--space-2);
    max-width: 640px;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .panel {
    overflow: hidden;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
    background: var(--color-surface-2);
  }
  /* its grey is lost on pure black: the edge marks where the rows are */
  :global(:root[data-black='true']) .panel {
    border-color: var(--color-border-strong);
  }
  /* flush with the panel, the same size as a page's help lines */
  .note {
    margin: 0;
    color: var(--color-text-muted);
    font-size: 13px;
    line-height: 1.45;
    overflow-wrap: anywhere;
  }
</style>
