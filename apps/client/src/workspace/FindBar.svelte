<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The bar over the open note that finds words in it: Enter and Shift+Enter
     or the arrows go from one match to the next, Escape leaves the caret on
     the match it was on. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'

  let {
    query = $bindable(),
    count,
    current,
    focus,
    onstep,
    onclose,
  }: {
    query: string
    count: number
    /** The match shown, from 0; -1 for none. */
    current: number
    /** Bumped to put the typing back in the bar, its words selected. */
    focus: number
    onstep: (by: 1 | -1) => void
    onclose: () => void
  } = $props()

  let input: HTMLInputElement | undefined = $state()

  $effect(() => {
    void focus
    input?.focus()
    input?.select()
  })

  function keydown(event: KeyboardEvent) {
    if (event.isComposing) return
    if (event.key === 'Enter') {
      event.preventDefault()
      onstep(event.shiftKey ? -1 : 1)
    } else if (event.key === 'Escape') {
      event.preventDefault()
      // the window's own Escape (a phone's back, a dialog) is not for this
      event.stopPropagation()
      onclose()
    }
  }
</script>

<div class="find" role="search" aria-label={t('find.label')}>
  <span class="lead"><Icon name="search" size={16} /></span>
  <input
    bind:this={input}
    bind:value={query}
    class="field"
    type="text"
    autocomplete="off"
    spellcheck="false"
    enterkeyhint="next"
    placeholder={t('find.placeholder')}
    aria-label={t('find.label')}
    onkeydown={keydown}
  />
  <span class="count muted" aria-live="polite">
    {#if query && count === 0}{t('find.none')}{:else if count > 0}{t('find.count', {
        current: current + 1,
        count,
      })}{/if}
  </span>
  {#if count > 0}
    <button class="icon-btn up" aria-label={t('find.previous')} onclick={() => onstep(-1)}>
      <Icon name="down" />
    </button>
    <button class="icon-btn" aria-label={t('find.next')} onclick={() => onstep(1)}>
      <Icon name="down" />
    </button>
  {/if}
  <button class="icon-btn" aria-label={t('common.close')} onclick={onclose}>
    <Icon name="close" />
  </button>
</div>

<style>
  .find {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-3) var(--space-2) var(--space-5);
    color: var(--color-text-muted);
    border-bottom: 1px solid var(--color-border);
  }
  .field {
    flex: 1;
    min-width: 0;
    color: var(--color-text);
  }
  /* the count does not push the buttons about as it grows */
  .count {
    min-width: 8ch;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }
  .lead {
    display: grid;
  }
  /* a phone's width goes to the words, the arrows keep their 44 px */
  @media (max-width: 40rem) {
    .find {
      gap: var(--space-1);
    }
    .lead {
      display: none;
    }
    .count {
      min-width: 0;
    }
  }
  .up :global(svg) {
    transform: rotate(180deg);
  }
  /* the matches in the note, in both views. The text inside takes the
     note's own colour: a link's red or a code colour on the tint would
     fall under 4.5:1. Named with their element, so they outrank the code
     colours on the same span */
  :global(:is(.prose span, .raw mark).find-match) {
    background: color-mix(in srgb, var(--color-warning) 30%, transparent);
    color: var(--color-text);
  }
  :global(:is(.prose span, .raw mark).find-current) {
    background: var(--color-accent);
    color: var(--color-accent-on);
  }
  :global(.find-match *) {
    color: inherit;
  }
</style>
