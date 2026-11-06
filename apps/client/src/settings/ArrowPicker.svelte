<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- What a typed -> becomes (editor/extensions/arrows.ts): off or one of three
     arrows. The desktop and phone settings give it its title. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { ARROW_CHOICES, prefs } from '../lib/prefs.svelte'
</script>

<div class="choices" role="radiogroup" aria-label={t('settings.arrow')}>
  {#each ARROW_CHOICES as arrow (arrow)}
    <button
      class="choice"
      class:off={!arrow}
      role="radio"
      aria-checked={prefs.arrow === arrow}
      onclick={() => (prefs.arrow = arrow)}>{arrow || t('settings.arrowOff')}</button
    >
  {/each}
</div>

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
  .choice.off {
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
</style>
