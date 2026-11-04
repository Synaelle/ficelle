<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- What a typed -> becomes (editor/extensions/arrows.ts): off, three
     arrows, or any other from the list of every Unicode arrow the app can
     draw (editor/arrows.json, made by tools/arrows-font.py). The desktop and
     phone settings give it its title. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import list from '../editor/arrows.json'
  import { t } from '../lib/i18n.svelte'
  import { modal } from '../lib/modal'
  import Icon from '../lib/icons/Icon.svelte'
  import { prefs } from '../lib/prefs.svelte'

  const CHOICES = ['', '→', '➔', '➞']
  const ARROWS = [...(list as string)]

  let open = $state(false)
  const other = $derived(!CHOICES.includes(prefs.arrow))

  function choose(arrow: string) {
    prefs.arrow = arrow
    open = false
  }
</script>

<div class="choices" role="radiogroup" aria-label={t('settings.arrow')}>
  {#each CHOICES as arrow (arrow)}
    <button
      class="choice"
      class:off={!arrow}
      role="radio"
      aria-checked={prefs.arrow === arrow}
      onclick={() => (prefs.arrow = arrow)}>{arrow || t('settings.arrowOff')}</button
    >
  {/each}
  <button
    class="choice"
    class:more={!other}
    role="radio"
    aria-checked={other}
    aria-label={t('settings.arrowMore')}
    title={t('settings.arrowMore')}
    onclick={() => (open = true)}
    >{#if other}{prefs.arrow}{:else}<Icon name="more" size={18} />{/if}</button
  >
</div>

{#if open}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && (open = false)}
  >
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="arrow-title"
      use:modal={{ onclose: () => (open = false) }}
    >
      <header>
        <h2 id="arrow-title">{t('settings.arrowPickerTitle')}</h2>
        <button class="icon-btn" aria-label={t('common.close')} onclick={() => (open = false)}
          ><Icon name="close" /></button
        >
      </header>
      <p class="muted">{t('settings.arrowFontsHelp', { app: brand.displayName })}</p>
      <div class="grid">
        {#each ARROWS as arrow (arrow)}
          <button
            class="arrow"
            class:current={prefs.arrow === arrow}
            aria-pressed={prefs.arrow === arrow}
            onclick={() => choose(arrow)}>{arrow}</button
          >
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .choice {
    display: grid;
    place-items: center;
    min-width: 48px;
    height: 40px;
    padding: 0 var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: none;
    color: var(--color-text);
    cursor: pointer;
    font-family: var(--font-editor);
    font-size: 20px;
  }
  /* across the whole group on a phone, each a finger's size */
  @media (pointer: coarse) {
    .choices {
      flex-wrap: nowrap;
    }
    .choice {
      flex: 1;
      min-width: 0;
      height: 44px;
      padding: 0;
    }
  }
  .choice.off,
  .choice.more {
    color: var(--color-text-muted);
    font-family: var(--font-ui);
    font-size: 13px;
  }
  .choice[aria-checked='true'] {
    border-color: var(--color-accent);
    color: var(--color-text);
  }
  .choice:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 1px;
  }

  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    gap: var(--space-3);
    width: min(520px, 100%);
    max-height: calc(100vh - 2 * var(--space-4));
    padding: var(--space-5);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
    background: var(--color-surface);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 16px;
  }
  .muted {
    margin: 0;
    color: var(--color-text-muted);
    font-size: 13px;
    line-height: 1.45;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(40px, 1fr));
    gap: var(--space-1);
    overflow-y: auto;
    min-height: 0;
  }
  .arrow {
    height: 40px;
    border: 1px solid transparent;
    border-radius: calc(var(--radius-popover) / 2);
    background: none;
    color: var(--color-text);
    cursor: pointer;
    font-family: var(--font-editor);
    font-size: 20px;
  }
  .arrow:hover {
    background: var(--color-surface-2);
  }
  .arrow.current {
    border-color: var(--color-accent);
  }
  .arrow:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: -2px;
  }
</style>
