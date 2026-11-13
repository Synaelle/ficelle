<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The web version's welcome, once per browser: what it is, where the notes
     stay, and the apps. A card over the dimmed notes, a sheet from the
     bottom on a phone; the knot ties itself as it arrives. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import Knot from '@app/ui/Knot.svelte'
  import { t } from './i18n.svelte'
  import Icon from './icons/Icon.svelte'
  import { modal } from './modal'
  import { prefs } from './prefs.svelte'

  /** The website, where the apps are: the host the releases list is on. */
  const site = new URL(brand.releasesUrl).origin

  const close = () => (prefs.webWelcomed = true)
</script>

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div
    class="welcome"
    role="dialog"
    aria-modal="true"
    aria-labelledby="welcome-title"
    aria-describedby="welcome-text"
    use:modal={{ onclose: close }}
  >
    <Knot motion="tie" size={76} />
    <div class="words">
      <h1 id="welcome-title">{t('web.welcomeTitle', { app: brand.displayName })}</h1>
      <div id="welcome-text">
        <p>{t('web.welcomeText')}</p>
        <p class="muted">{t('web.welcomeApps')}</p>
      </div>
    </div>
    <div class="actions">
      <button class="btn primary" type="button" data-autofocus="" onclick={close}
        >{t('web.welcomeStart')}</button
      >
      <a class="btn" href={site} target="_blank" rel="noopener"
        >{t('web.welcomeGetApps')}<Icon name="export" size={14} /></a
      >
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    /* the room dims, the notes stay faintly behind */
    background: rgb(8 6 6 / 0.72);
    animation: dim 220ms cubic-bezier(0.2, 0.7, 0.2, 1);
  }
  .welcome {
    width: min(560px, 100%);
    box-sizing: border-box;
    display: grid;
    gap: var(--space-5);
    justify-items: start;
    padding: 36px 40px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-window);
    background: var(--color-surface);
    box-shadow: var(--shadow-dialog);
    animation: arrive 360ms cubic-bezier(0.2, 0.7, 0.2, 1);
  }
  .welcome :global(.knot) {
    margin-left: -6px;
  }
  .words {
    display: grid;
    gap: var(--space-3);
  }
  h1 {
    margin: 0;
    font-family: var(--font-editor);
    font-size: 30px;
    line-height: 1.2;
    font-weight: 700;
    color: var(--color-text);
  }
  #welcome-text {
    display: grid;
    gap: var(--space-3);
  }
  p {
    margin: 0;
    font-family: var(--font-editor);
    font-size: 16px;
    line-height: 1.65;
    color: var(--color-text);
  }
  p.muted {
    color: var(--color-text-muted);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding-top: var(--space-2);
  }
  .actions a {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text);
    text-decoration: none;
  }
  .actions a :global(svg) {
    color: var(--color-text-muted);
  }

  /* a phone: a sheet from the bottom, as the app shows its prompts */
  @media (max-width: 520px) {
    .backdrop {
      place-items: end stretch;
      padding: 0;
    }
    .welcome {
      width: 100%;
      gap: 18px;
      padding: 32px 24px calc(32px + env(safe-area-inset-bottom));
      border-width: 1px 0 0;
      border-radius: var(--radius-window) var(--radius-window) 0 0;
      animation-name: rise;
    }
    .welcome :global(.knot) {
      margin-left: -5px;
    }
    h1 {
      font-size: 28px;
    }
    p {
      line-height: 1.6;
    }
    .actions {
      flex-direction: column;
      align-self: stretch;
      width: 100%;
    }
    .actions > * {
      justify-content: center;
    }
  }

  @keyframes dim {
    from {
      opacity: 0;
    }
  }
  /* moves only: a paused animation never leaves the card unseen */
  @keyframes arrive {
    from {
      transform: translateY(8px);
    }
  }
  @keyframes rise {
    from {
      transform: translateY(100%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .backdrop,
    .welcome {
      animation: none;
    }
  }
</style>
