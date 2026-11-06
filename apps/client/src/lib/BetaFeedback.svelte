<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Beta builds only: the feedback button floating bottom right (phones have
     it in the notes list instead), the real feedback form in a dialog, and
     the welcome shown once, with the Mac warning merged in on a Mac. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { channelAskDue } from './appUpdate.svelte'
  import { betaFeedback, isBeta, isMacApp, touchScreen } from './beta.svelte'
  import { t } from './i18n.svelte'
  import Icon from './icons/Icon.svelte'
  import { modal } from './modal'
  import { onBack } from './back'
  import { prefs } from './prefs.svelte'
  import FeedbackSettings from '../settings/FeedbackSettings.svelte'

  // after the one-time channel question, never on top of it
  const welcome = $derived(isBeta && !prefs.betaWelcomed && !channelAskDue())

  const close = () => (betaFeedback.open = false)

  $effect(() => {
    if (betaFeedback.open) return onBack(close)
  })
</script>

{#if isBeta}
  {#if !touchScreen}
    <button
      class="launcher"
      class:beside={__APP_DEV_TOOLS__}
      title={t('beta.feedback')}
      aria-label={t('beta.feedback')}
      data-no-shot
      onclick={() => (betaFeedback.open = true)}
    >
      <Icon name="speech" size={16} />
    </button>
  {/if}

  {#if betaFeedback.open}
    <div
      class="backdrop"
      role="presentation"
      onclick={(e) => e.target === e.currentTarget && close()}
    >
      <div
        class="dialog feedback"
        role="dialog"
        aria-modal="true"
        aria-label={t('beta.feedback')}
        use:modal={{ onclose: close }}
      >
        <div class="head">
          <h2>{t('beta.feedback')}</h2>
          <button class="icon-btn" type="button" aria-label={t('common.close')} onclick={close}
            ><Icon name="close" /></button
          >
        </div>
        <FeedbackSettings />
      </div>
    </div>
  {:else if welcome}
    <div class="backdrop" role="presentation">
      <div
        class="dialog"
        role="alertdialog"
        aria-modal="true"
        aria-label={t('beta.welcomeLabel')}
        aria-describedby="beta-welcome"
        use:modal
      >
        <div id="beta-welcome" class="text">
          <p>{touchScreen ? t('beta.welcomePhone') : t('beta.welcome')}</p>
          {#if isMacApp}<p>{t('beta.mac', { address: brand.contactEmail })}</p>{/if}
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
{/if}

<style>
  .launcher {
    position: fixed;
    right: var(--space-3);
    bottom: var(--space-3);
    z-index: 40;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-chip);
    border: 1px solid var(--color-border-strong);
    background: var(--color-surface-2);
    color: var(--color-text-muted);
    cursor: pointer;
  }
  /* the dev report button is at the corner in dev builds */
  .launcher.beside {
    right: calc(var(--space-3) + 40px);
  }
  .launcher:focus-visible {
    color: var(--color-accent-text);
  }
  @media (hover: hover) {
    .launcher:hover {
      color: var(--color-accent-text);
    }
  }
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
    box-shadow: 0 8px 32px rgb(0 0 0 / 0.35);
  }
  /* on the page's colour, so its groups stand out as they do in settings */
  .dialog.feedback {
    width: min(560px, 100%);
    background: var(--color-bg);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  h2 {
    margin: 0;
    font-size: 16px;
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
