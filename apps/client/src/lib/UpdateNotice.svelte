<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A newer version is out: a card above the toasts, the knot tying itself
     beside the version, the button that downloads it (its progress below,
     the app usable meanwhile) then installs it, what changed behind "What's
     changed". "Later" offers waiting for the next start or
     skipping this version for the next one, the cross skips it at once
     (a security fix can only wait). -->
<script lang="ts">
  import ErrorText from './ErrorText.svelte'
  import { ANDROID, PHONE } from './platform'
  import {
    appUpdates,
    changelogLabelled,
    downloadLine,
    isFix,
    updateLines,
  } from './appUpdate.svelte'
  import { channelSwitch } from './channelChange.svelte'
  import Icon from './icons/Icon.svelte'
  import Knot from '@app/ui/Knot.svelte'
  import { t } from './i18n.svelte'

  const phone = ANDROID
  let changes = $state(false)

  /** A new version (another middle number, or a beta) or a fix for this one,
   * downloaded or only out (`page`: the portable exe, a Linux package). */
  const sub = $derived.by(() => {
    const ready = appUpdates.ready
    if (!ready) return ''
    const fix = isFix(ready.version)
    if (!appUpdates.downloaded) return fix ? t('appUpdate.cardFixOut') : t('appUpdate.cardNewOut')
    if (appUpdates.installed) return t('appUpdate.cardInstalled')
    return fix ? t('appUpdate.cardFixReady') : t('appUpdate.cardNewReady')
  })

  /** What the release is about, translated. */
  const lines = $derived(appUpdates.ready ? updateLines(appUpdates.ready) : [])
  /** Its list of changes in English; a release before the messages only
   * has its English line. */
  const details = $derived(appUpdates.ready?.changelog || appUpdates.ready?.notes || '')
</script>

<!-- a phone has its mark in the note list instead -->
{#if appUpdates.ready && appUpdates.offered && !PHONE}
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
      <!-- skips this version; a security fix cannot be skipped, it only waits -->
      <button
        class="icon-btn close"
        aria-label={appUpdates.ready.critical ? t('appUpdate.later') : t('appUpdate.skip')}
        title={appUpdates.ready.critical ? t('appUpdate.later') : t('appUpdate.skip')}
        onclick={() => appUpdates.close()}><Icon name="close" size={14} /></button
      >
    </div>
    {#if lines.length}<p class="lines">{lines.join(' ')}</p>{/if}
    {#if details}
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
        <div class="notes" id="update-notes" tabindex="0">
          {#if changelogLabelled()}<span class="lang">{t('appUpdate.message.inEnglish')}</span>{/if}
          <p lang="en">{details}</p>
        </div>
      {/if}
    {/if}
    {#if appUpdates.progress}
      <div class="progress" role="status">
        <progress
          max={appUpdates.progress.total ?? undefined}
          value={appUpdates.progress.total ? appUpdates.progress.done : undefined}
        ></progress>
        <span>{downloadLine(appUpdates.progress)}</span>
      </div>
    {/if}
    {#if appUpdates.error}<p class="error"><ErrorText error={appUpdates.error} /></p>{/if}
    <div class="actions">
      <button
        class="btn primary"
        disabled={appUpdates.installing || !!appUpdates.progress}
        onclick={() => void channelSwitch.update()}
        >{appUpdates.ready.page || !appUpdates.downloaded
          ? t('appUpdate.download')
          : appUpdates.pacman
            ? appUpdates.installed
              ? appUpdates.installing
                ? t('appUpdate.restarting')
                : t('appUpdate.restartNow')
              : appUpdates.installing
                ? t('appUpdate.installing')
                : t('appUpdate.install')
            : appUpdates.installing
              ? t('appUpdate.restarting')
              : phone
                ? t('appUpdate.install')
                : t('appUpdate.restart')}</button
      >
      <button
        class="btn later"
        aria-haspopup={appUpdates.ready.critical ? undefined : 'menu'}
        onclick={(e) => appUpdates.later(e.currentTarget)}>{t('appUpdate.later')}</button
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
    box-shadow: var(--shadow-popover);
    pointer-events: auto;
    animation: update-in var(--dur-slow) var(--ease-out) both;
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
    flex: 1;
    min-width: 0;
  }
  .close {
    align-self: flex-start;
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
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .chev.open {
    transform: rotate(180deg);
  }
  .lines {
    margin: 0;
    color: var(--color-text);
    line-height: 1.45;
  }
  .notes p {
    margin: 0;
  }
  .lang {
    display: block;
    margin-bottom: var(--space-1);
    font-size: 12px;
    font-weight: 600;
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
  .progress {
    display: grid;
    gap: var(--space-1);
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .progress progress {
    width: 100%;
    height: 6px;
    accent-color: var(--color-accent);
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
