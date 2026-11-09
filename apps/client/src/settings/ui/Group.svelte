<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A group of settings rows on the desktop, like the phone's: a rounded
     panel, an optional title above and a short note below. -->
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
    margin: 0 var(--space-1);
    font-size: 13px;
    font-weight: 600;
  }
  .panel {
    overflow: hidden;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
    background: var(--color-surface-2);
  }
  .note {
    margin: 0 var(--space-1);
    color: var(--color-text-muted);
    font-size: 12px;
    line-height: 1.45;
    overflow-wrap: anywhere;
  }
</style>
