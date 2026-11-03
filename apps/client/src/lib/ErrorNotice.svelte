<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- An error nothing handled: a card in the toasts' corner offering to
     report it (lib/errorCatch). -->
<script lang="ts">
  import { errorCatch } from './errorCatch.svelte'
  import Icon from './icons/Icon.svelte'
  import { t } from './i18n.svelte'
</script>

{#if errorCatch.caught && !errorCatch.open && !errorCatch.shooting}
  <div class="notice" data-error-notice data-no-shot>
    <span class="text">{t('errorReport.notice')}</span>
    <button class="btn small" onclick={() => void errorCatch.report()}
      >{t('errorReport.report')}</button
    >
    <button class="icon-btn" aria-label={t('common.close')} onclick={() => errorCatch.dismiss()}
      ><Icon name="close" size={14} /></button
    >
  </div>
{/if}

<style>
  /* in the toasts' corner, which places it */
  .notice {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
    background: var(--color-surface-2);
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.25);
    pointer-events: auto;
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .small {
    padding: 2px var(--space-2);
    font-size: 12px;
  }
</style>
