<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Note titles offered after typing [[. Arrow keys move, Enter or Tab picks. -->
<script lang="ts">
  import type { SuggestionProps } from '@tiptap/suggestion'
  import { t } from '../lib/i18n.svelte'
  import type { WikiItem } from './extensions/wikiLink'

  let { props }: { props: SuggestionProps<WikiItem> } = $props()

  let index = $state(0)
  $effect(() => {
    void props.items
    index = 0
  })

  const box = $derived(props.clientRect?.())
  const style = $derived(box ? `top:${box.bottom + 6}px;left:${box.left}px` : 'display:none')

  /** Returns true when the key was used, so the editor ignores it. */
  export function key(name: string): boolean {
    const count = props.items.length
    if (name === 'ArrowDown' && count) return ((index = (index + 1) % count), true)
    if (name === 'ArrowUp' && count) return ((index = (index - 1 + count) % count), true)
    if ((name === 'Enter' || name === 'Tab') && count) {
      const item = props.items[index]
      if (item) props.command(item)
      return true
    }
    return false
  }
</script>

<ul class="suggest" {style} role="listbox" aria-label={t('editor.linkSuggestions')}>
  {#each props.items as item, i (`${item.title}#${item.heading ?? ''}`)}
    <li role="option" aria-selected={i === index}>
      <button onmousedown={(e) => e.preventDefault()} onclick={() => props.command(item)}
        >{item.heading ? `${item.title} › ${item.heading}` : item.title}</button
      >
    </li>
  {:else}
    <li class="empty">{t('editor.noLinkMatch')}</li>
  {/each}
</ul>

<style>
  .suggest {
    position: fixed;
    z-index: 30;
    min-width: 220px;
    max-width: 360px;
    margin: 0;
    padding: var(--space-1);
    list-style: none;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
  }
  button {
    width: 100%;
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  [aria-selected='true'] button {
    background: var(--color-selection);
  }
  .empty {
    padding: var(--space-1) var(--space-2);
    color: var(--color-text-muted);
  }
</style>
