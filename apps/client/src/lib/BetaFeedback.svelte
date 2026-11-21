<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Nightly builds: the welcome shown once. Feedback is in Settings. -->
<script lang="ts">
  import { isBeta } from './beta.svelte'
  import { WEB } from './platform'
  import { t } from './i18n.svelte'
  import { modal } from './modal'
  import { prefs } from './prefs.svelte'

  // the web version has its own welcome (WebWelcome)
  const welcome = $derived(isBeta && !WEB && !prefs.betaWelcomed)
</script>

{#if welcome}
  <div class="backdrop" role="presentation">
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-label={t('beta.nightlyWelcomeLabel')}
      aria-describedby="beta-welcome"
      use:modal
    >
      <div id="beta-welcome" class="text">
        <p>{t('beta.nightlyWelcome')}</p>
      </div>
      <div class="actions">
        <button
          class="btn primary"
          type="button"
          data-autofocus=""
          onclick={() => (prefs.betaWelcomed = true)}>{t('common.ok')}</button
        >
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    width: min(420px, 100%);
    max-height: 100%;
    overflow: auto;
    display: grid;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
    background: var(--color-surface-2);
    box-shadow: var(--shadow-dialog);
  }
  .text {
    display: grid;
    gap: var(--space-3);
  }
  p {
    margin: 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
