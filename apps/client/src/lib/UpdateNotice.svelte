<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A newer version is downloaded: a card above the toasts with the button
     that installs it, what changed behind "What's changed". Closing it waits
     for the next start. -->
<script lang="ts">
  import { appUpdates } from './appUpdate.svelte'
  import Icon from './icons/Icon.svelte'
  import { t } from './i18n.svelte'

  const phone = !!window.AppInstaller
  let changes = $state(false)
</script>

{#if appUpdates.ready && appUpdates.shown && !appUpdates.dismissed && !appUpdates.critical}
  <div class="update">
    <div class="head">
      <strong
        >{appUpdates.ready.page
          ? t('appUpdate.out', { version: appUpdates.ready.version })
          : t('appUpdate.ready', { version: appUpdates.ready.version })}</strong
      >
      <button
        class="icon-btn"
        aria-label={t('appUpdate.later')}
        onclick={() => (appUpdates.dismissed = true)}><Icon name="close" size={14} /></button
      >
    </div>
    {#if appUpdates.ready.notes}
      <button class="changes" aria-expanded={changes} onclick={() => (changes = !changes)}
        >{t('appUpdate.changes')}</button
      >
      {#if changes}<p class="notes">{appUpdates.ready.notes}</p>{/if}
    {/if}
    {#if appUpdates.error}<p class="error">{t(`errors.${appUpdates.error}`)}</p>{/if}
    <button
      class="btn primary"
      disabled={appUpdates.installing}
      onclick={() => void appUpdates.install()}
      >{appUpdates.ready.page
        ? t('appUpdate.download')
        : appUpdates.installing
          ? t('appUpdate.restarting')
          : phone
            ? t('appUpdate.install')
            : t('appUpdate.restart')}</button
    >
  </div>
{/if}

<style>
  /* in the toasts' corner, which places it */
  .update {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
    background: var(--color-surface-2);
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.25);
    pointer-events: auto;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  p {
    margin: 0;
    font-size: 13px;
  }
  .changes {
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-accent-text);
    cursor: pointer;
    font: inherit;
    font-size: 13px;
  }
  .changes:hover {
    text-decoration: underline;
  }
  .notes {
    color: var(--color-text-muted);
    overflow-wrap: anywhere;
  }
  .error {
    color: var(--color-danger);
  }
  .btn {
    justify-content: center;
  }
</style>
