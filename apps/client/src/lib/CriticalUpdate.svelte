<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A critical security fix is out: asked in the middle of the screen, with
     or without automatic updates. Nothing installs without "Update now";
     "Don't update" asks once more, then this version is not asked again. -->
<script lang="ts">
  import { appUpdates } from './appUpdate.svelte'
  import { t } from './i18n.svelte'
  import { modal } from './modal'
  import { overlay } from './overlay.svelte'

  const phone = !!window.AppInstaller
  let busy = $state(false)

  async function update() {
    busy = true
    try {
      await appUpdates.updateCritical()
    } finally {
      busy = false
    }
  }

  async function skip() {
    const sure = await overlay.ask(t('appUpdate.criticalSure'), {
      confirmLabel: t('appUpdate.criticalSkipConfirm'),
      danger: true,
    })
    if (sure) appUpdates.refuseCritical()
  }
</script>

{#if appUpdates.critical && appUpdates.shown && !overlay.dialog}
  <div class="backdrop" role="presentation">
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="critical-title"
      aria-describedby="critical-text"
      use:modal
    >
      <h2 id="critical-title">{t('appUpdate.criticalTitle')}</h2>
      <p id="critical-text">
        {t('appUpdate.criticalText', { version: appUpdates.critical.version })}
      </p>
      {#if appUpdates.error}<p class="error" role="alert">{t(`errors.${appUpdates.error}`)}</p>{/if}
      <div class="actions">
        <button class="btn" type="button" disabled={busy} onclick={() => void skip()}
          >{t('appUpdate.criticalSkip')}</button
        >
        <button
          class="btn primary"
          type="button"
          data-autofocus=""
          disabled={busy || appUpdates.installing}
          onclick={() => void update()}
          >{busy || appUpdates.installing
            ? phone
              ? t('appUpdate.checking')
              : t('appUpdate.restarting')
            : t('appUpdate.criticalUpdate')}</button
        >
      </div>
    </div>
  </div>
{/if}

<style>
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
    width: min(440px, 100%);
    display: grid;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
    background: var(--color-surface-2);
    box-shadow: 0 8px 32px rgb(0 0 0 / 0.35);
  }
  h2 {
    margin: 0;
    font-size: 1.15rem;
  }
  p {
    margin: 0;
  }
  .error {
    color: var(--color-danger);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
</style>
