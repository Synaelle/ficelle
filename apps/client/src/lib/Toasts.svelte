<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The messages of lib/toast, read out by screen readers as they come, and
     the app update and error cards on top of them. -->
<script lang="ts">
  import ErrorNotice from './ErrorNotice.svelte'
  import Icon from './icons/Icon.svelte'
  import UpdateNotice from './UpdateNotice.svelte'
  import { t } from './i18n.svelte'
  import { toasts } from './toast.svelte'
</script>

<div class="toasts" role="status" aria-live="polite">
  <UpdateNotice />
  <ErrorNotice />
  {#each toasts.list as toast (toast.id)}
    <div class="toast">
      <span class="text"><b>{toast.from}</b> {toast.text}</span>
      {#if toast.action}
        {@const action = toast.action}
        <button
          class="btn small"
          onclick={() => {
            toasts.dismiss(toast.id)
            action.run()
          }}>{action.label}</button
        >
      {/if}
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
  /* on touch screens an open note has its toolbar at the bottom: above it */
  @media (pointer: coarse) {
    :global(body:has(.editor .toolbar)) .toasts {
      bottom: calc(var(--space-4) + 64px);
    }
  }
  .toast {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
    background: var(--color-surface-2);
    box-shadow: var(--shadow-popover);
    pointer-events: auto;
  }
  /* the message keeps room for a few words, else the buttons go under it */
  .text {
    flex: 1 1 12rem;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .small {
    padding: 2px var(--space-2);
    font-size: 12px;
  }
  @media (pointer: coarse) {
    .small {
      min-height: 44px;
      padding: 0 var(--space-3);
    }
  }
</style>
