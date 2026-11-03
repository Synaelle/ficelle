<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Each step of changing channel (lib/channelChange.svelte.ts): the backup
     question before joining Beta or going back to Stable, the warning before
     going back, why it has to wait, the download, and on Android the steps
     through an uninstall. A scheduled backup runs on the spot; without one,
     the one-file backup opens and the switch waits for it to be made.
     Switching without a backup is offered only when the notes are on the
     sync server and nothing waits to upload. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { brand } from '@app/brand'
  import { appUpdates, thisVersion } from './appUpdate.svelte'
  import { backups } from './backups.svelte'
  import { pretendRelease } from './beta.svelte'
  import { channelSwitch, notesSynced } from './channelChange.svelte'
  import { t } from './i18n.svelte'
  import { modal } from './modal'
  import { overlay } from './overlay.svelte'
  import BackupDialog from '../settings/BackupDialog.svelte'

  const WAIT_SECONDS = 5

  let busy = $state(false)
  let problem = $state('')
  let oneFile = $state(false)
  let left = $state(WAIT_SECONDS)

  const android = !!window.AppInstaller
  const safety = $derived(notesSynced())
  const step = $derived(channelSwitch.step)
  const back = $derived(step === 'backup')
  const jump = $derived(step === 'bigJump')
  const version = $derived(channelSwitch.target?.version ?? '')
  const file = $derived(channelSwitch.saved.split('/').pop() ?? '')
  /** Android: where backups go, which may stay after an uninstall. */
  const backupsKept = android && (window.AppInstaller?.backupsKept?.() ?? false)
  const backupFolder = `${brand.displayName} backups`

  // the warning's button waits a few seconds, so it is read first
  onMount(() => {
    const timer = setInterval(() => {
      if (channelSwitch.step === 'warn') left = Math.max(0, left - 1)
    }, 1000)
    return () => clearInterval(timer)
  })

  async function backUp() {
    problem = ''
    if (!backups.view?.config) {
      oneFile = true
      return
    }
    busy = true
    const started = Date.now()
    try {
      await backups.run()
      const done = backups.view?.status.lastSuccessAt ?? 0
      if (done < started - 1000) throw new Error(backups.view?.status.lastError ?? '')
      await channelSwitch.proceed(true)
    } catch (err) {
      problem = safety === 'synced' ? t('betaSwitch.failed') : t('betaSwitch.failedTryAgain')
      console.warn('backup before changing channel failed', err)
    } finally {
      busy = false
    }
  }

  async function uninstall() {
    const yes = await overlay.ask(t('backToStable.uninstallSure', { appName: brand.displayName }), {
      confirmLabel: t('backToStable.uninstall', { appName: brand.displayName }),
      danger: true,
    })
    if (!yes) return
    // a pretend beta (Test tools) uninstalls nothing
    if (pretendRelease) problem = t('errors.testBuild')
    else window.AppInstaller?.uninstall?.()
  }

  const busyNow = $derived(busy || step === 'going')
  const cancel = () => !busyNow && channelSwitch.close()
</script>

{#if oneFile}
  <BackupDialog
    mode="make"
    onmade={() => {
      oneFile = false
      void channelSwitch.proceed(true)
    }}
    close={() => (oneFile = false)}
  />
{:else}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && cancel()}
  >
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="channel-switch-text"
      aria-busy={busyNow}
      use:modal={{ onclose: cancel }}
    >
      {#if step === 'join' || step === 'backup' || step === 'bigJump'}
        <p id="channel-switch-text">
          {jump
            ? t('bigJump.message', {
                version: appUpdates.ready?.version ?? '',
                current: thisVersion,
              })
            : back
              ? t('backToStable.backupMessage')
              : t('betaSwitch.message')}
        </p>
        {#if safety === 'synced'}
          <p class="muted">{t('betaSwitch.synced')}</p>
        {:else if safety === 'uploading'}
          <p class="muted">{t('betaSwitch.uploading')}</p>
        {/if}
        {#if back && android}
          <p class="muted">
            {backupsKept
              ? t('backToStable.backupKept', { folder: backupFolder })
              : t('backToStable.backupLost')}
          </p>
        {/if}
        {#if problem}<p class="error" role="alert">{problem}</p>{/if}
        <div class="actions">
          <button class="btn" disabled={busy} onclick={cancel}>{t('common.cancel')}</button>
          {#if safety === 'synced'}
            <button class="btn" disabled={busy} onclick={() => void channelSwitch.proceed(false)}
              >{jump ? t('bigJump.skip') : t('betaSwitch.skip')}</button
            >
          {/if}
          <button class="btn primary" disabled={busy} onclick={() => void backUp()}
            >{busy
              ? t('betaSwitch.backingUp')
              : jump
                ? t('bigJump.backUp')
                : t('betaSwitch.backUp')}</button
          >
        </div>
      {:else if step === 'warn'}
        <p id="channel-switch-text">{t('backToStable.warning')}</p>
        <div class="actions">
          <button class="btn" onclick={cancel}>{t('common.cancel')}</button>
          <button class="btn" onclick={() => channelSwitch.waitForStable()}
            >{t('backToStable.wait')}</button
          >
          <button class="btn danger" disabled={left > 0} onclick={() => channelSwitch.confirmBack()}
            >{t('backToStable.switch')}{left > 0 ? ` (${left})` : ''}</button
          >
        </div>
      {:else if step === 'schema'}
        <p id="channel-switch-text">{t('backToStable.schema')}</p>
        <div class="actions">
          <button class="btn" onclick={cancel}>{t('common.cancel')}</button>
          <button class="btn primary" onclick={() => channelSwitch.waitForStable()}
            >{t('backToStable.wait')}</button
          >
        </div>
      {:else if step === 'going'}
        <p id="channel-switch-text" role="status">
          {t('backToStable.downloading', { version })}
        </p>
      {:else if step === 'testBuild'}
        <p id="channel-switch-text" role="status">{t('errors.testBuild')}</p>
        <div class="actions">
          <button class="btn primary" onclick={cancel}>{t('common.close')}</button>
        </div>
      {:else if step === 'failed'}
        <p id="channel-switch-text" class="error" role="alert">{t('backToStable.failed')}</p>
        {#if channelSwitch.error}<p class="muted">{t(`errors.${channelSwitch.error}`)}</p>{/if}
        <div class="actions">
          <button class="btn primary" onclick={cancel}>{t('common.close')}</button>
        </div>
      {:else if step === 'android'}
        <h2 id="channel-switch-text">{t('backToStable.androidTitle', { version })}</h2>
        {#if pretendRelease}<p class="error">{t('errors.testBuild')}</p>{/if}
        <p class="muted">
          {file
            ? t('backToStable.androidSaved', { file })
            : t('backToStable.androidNotSaved', { version })}
        </p>
        <ol>
          <li>
            {t('backToStable.stepUninstall', { appName: brand.displayName })}
            <button class="btn danger" onclick={() => void uninstall()}
              >{t('backToStable.uninstall', { appName: brand.displayName })}</button
            >
          </li>
          <li>
            {file
              ? t('backToStable.stepInstall', { file })
              : t('backToStable.stepInstallDownloaded')}
          </li>
          {#if safety !== 'none'}
            <li>{t('backToStable.stepSignIn')}</li>
          {/if}
          {#if channelSwitch.backedUp || safety === 'none'}
            <li>
              {backupsKept
                ? t('backToStable.stepRestore', { folder: backupFolder })
                : t('backToStable.stepRestoreCopied')}
            </li>
          {/if}
        </ol>
        {#if problem}<p class="error" role="alert">{problem}</p>{/if}
        <div class="actions">
          <button class="btn" onclick={cancel}>{t('backToStable.later')}</button>
        </div>
      {/if}
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
    width: min(460px, 100%);
    max-height: calc(100dvh - 2 * var(--space-4));
    overflow-y: auto;
    display: grid;
    gap: var(--space-3);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  p {
    margin: 0;
  }
  ol {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding-left: var(--space-5);
  }
  li .btn {
    display: flex;
    margin-top: var(--space-2);
  }
  .error {
    color: var(--color-danger);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .danger:disabled {
    opacity: 0.6;
  }
  /* a finger's reach: one button per line, the main one first */
  @media (pointer: coarse) {
    .actions {
      flex-direction: column-reverse;
    }
    .actions .btn,
    li .btn {
      width: 100%;
      justify-content: center;
    }
  }
</style>
