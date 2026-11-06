<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A newer version is downloaded: a card above the toasts, the knot tying
     itself beside the version, the button that installs it, what changed
     behind "What's changed". "Later" offers waiting for the next start or
     skipping this version for the next one (a security fix can only wait). -->
<script lang="ts">
  import { ANDROID } from './platform'
  import { appUpdates, isFix } from './appUpdate.svelte'
  import { channelSwitch } from './channelChange.svelte'
  import Icon from './icons/Icon.svelte'
  import Knot from './Knot.svelte'
  import { t } from './i18n.svelte'
  import { overlay } from './overlay.svelte'

  const phone = ANDROID
  let changes = $state(false)

  function later(event: MouseEvent) {
    const wait = () => (appUpdates.dismissed = true)
    if (appUpdates.ready?.critical) return void wait()
    overlay.openMenuBelow(event.currentTarget as HTMLElement, [
      { label: t('appUpdate.laterStart'), icon: 'timer', run: wait },
      { label: t('appUpdate.skip'), icon: 'close', run: () => appUpdates.skip() },
    ])
  }

  /** A new version (another middle number, or a beta) or a fix for this one,
   * downloaded or only out (`page`: the portable exe, sometimes the Mac app). */
  const sub = $derived.by(() => {
    const ready = appUpdates.ready
    if (!ready) return ''
    const fix = isFix(ready.version)
    if (ready.page) return fix ? t('appUpdate.cardFixOut') : t('appUpdate.cardNewOut')
    return fix ? t('appUpdate.cardFixReady') : t('appUpdate.cardNewReady')
  })
</script>

{#if appUpdates.ready && appUpdates.shown && !appUpdates.dismissed && !appUpdates.critical && appUpdates.skipped !== appUpdates.ready.version}
  <section
    class="update"
    aria-label={t('appUpdate.cardTitle', { version: appUpdates.ready.version })}
  >
    <div class="head">
      <span class="mark"><Knot motion="tie" size={32} /></span>
      <div class="titles">
        <strong>{t('appUpdate.cardTitle', { version: appUpdates.ready.version })}</strong>
        <span class="sub">{sub}</span>
      </div>
    </div>
    {#if appUpdates.ready.notes}
      <button
        class="changes"
        aria-expanded={changes}
        aria-controls="update-notes"
        onclick={() => (changes = !changes)}
      >
        {t('appUpdate.changes')}
        <span class="chev" class:open={changes}><Icon name="down" size={14} /></span>
      </button>
      {#if changes}
        <!-- focusable, so the keyboard can scroll long notes -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <p class="notes" id="update-notes" tabindex="0">{appUpdates.ready.notes}</p>
      {/if}
    {/if}
    {#if appUpdates.error}<p class="error">{t(`errors.${appUpdates.error}`)}</p>{/if}
    <div class="actions">
      <button
        class="btn later"
        aria-haspopup={appUpdates.ready.critical ? undefined : 'menu'}
        onclick={later}>{t('appUpdate.later')}</button
      >
      <button
        class="btn primary"
        disabled={appUpdates.installing}
        onclick={() => void channelSwitch.update()}
        >{appUpdates.ready.page
          ? t('appUpdate.download')
          : appUpdates.installing
            ? t('appUpdate.restarting')
            : phone
              ? t('appUpdate.install')
              : t('appUpdate.restart')}</button
      >
    </div>
  </section>
{/if}

<style>
  /* in the toasts' corner, which places it */
  .update {
    --badge: color-mix(in srgb, var(--color-accent) 14%, var(--color-surface-2));
    display: grid;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
    background: var(--color-surface-2);
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.25);
    pointer-events: auto;
    animation: update-in 320ms cubic-bezier(0.2, 0.8, 0.3, 1) both;
  }
  @keyframes update-in {
    from {
      opacity: 0;
      transform: translateY(10px);
    }
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .mark {
    --knot-behind: var(--badge);
    display: grid;
    flex: none;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: var(--radius-chip);
    background: var(--badge);
  }
  .titles {
    display: grid;
    min-width: 0;
  }
  strong {
    font-size: 15px;
    overflow-wrap: anywhere;
  }
  .sub {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  p {
    margin: 0;
    font-size: 13px;
  }
  .changes {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    justify-self: start;
    min-height: 28px;
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
  .chev {
    display: inline-flex;
    transition: transform 160ms ease;
  }
  .chev.open {
    transform: rotate(180deg);
  }
  .notes {
    max-height: 9em;
    overflow-y: auto;
    padding: var(--space-2) var(--space-3);
    border-radius: calc(var(--radius-popover) / 2);
    background: color-mix(in srgb, var(--color-text) 6%, transparent);
    color: var(--color-text-muted);
    line-height: 1.45;
    overflow-wrap: anywhere;
    white-space: pre-line;
  }
  .error {
    color: var(--color-danger);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    justify-content: flex-end;
  }
  .btn {
    justify-content: center;
  }
  .later {
    border-color: transparent;
    background: none;
    color: var(--color-text-muted);
  }
  .later:hover {
    color: var(--color-text);
  }
  .primary {
    flex: 1;
  }
  @media (prefers-reduced-motion: reduce) {
    .update {
      animation: none;
    }
    .chev {
      transition: none;
    }
  }
</style>
