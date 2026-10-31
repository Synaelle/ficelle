<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The messages of lib/toast, read out by screen readers as they come, and
     the app update card on top of them. -->
<script lang="ts">
  import Icon from './icons/Icon.svelte'
  import UpdateNotice from './UpdateNotice.svelte'
  import { t } from './i18n.svelte'
  import { toasts } from './toast.svelte'
</script>

<div class="toasts" role="status" aria-live="polite">
  <UpdateNotice />
  {#each toasts.list as toast (toast.id)}
    <div class="toast">
      <span class="text"><b>{toast.from}</b> {toast.text}</span>
      <button
        class="icon-btn"
        aria-label={t('common.close')}
        onclick={() => toasts.dismiss(toast.id)}><Icon name="close" size={14} /></button
      >
    </div>
  {/each}
</div>

<style>
  /* in a corner and under dialogs: a message never covers a choice */
  .toasts {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-4);
    z-index: 50;
    display: grid;
    gap: var(--space-2);
    width: min(340px, calc(100vw - 2 * var(--space-4)));
    pointer-events: none;
  }
  .toast {
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
    overflow-wrap: anywhere;
  }
</style>
