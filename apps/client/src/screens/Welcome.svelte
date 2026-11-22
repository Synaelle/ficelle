<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The first start of a new install, its notes made already and not
     encrypted: two questions, then the notes. Automatic backups go to the
     place Data and backups would offer, the phone's own backups folder or
     Documents on a computer. Encryption goes through the password and
     recovery key steps of Settings, Security. -->
<script lang="ts">
  import { documentDir } from '@tauri-apps/api/path'
  import Knot from '@app/ui/Knot.svelte'
  import { onBack } from '../lib/back'
  import { backend } from '../lib/backend'
  import { backupError, backups } from '../lib/backups.svelte'
  import { t } from '../lib/i18n.svelte'
  import Setup from './Setup.svelte'

  let { done }: { done: () => void } = $props()

  type Step = 'backups' | 'encrypt' | 'password'
  let step: Step = $state('backups')
  let busy = $state(false)
  let error = $state('')
  /** Encrypted by the password steps: leaving them opens the notes. */
  let encrypted = false

  $effect(() => {
    if (step === 'encrypt') return onBack(() => (step = 'backups'))
  })

  async function answerBackups(yes: boolean) {
    busy = true
    error = ''
    try {
      if (yes) {
        const folder = window.AppSystem?.backupFolder?.() || (await documentDir())
        // the first backup comes with the schedule once the notes open,
        // sealed with the password if one is chosen next
        backups.view = await backend.backupsTurnOn(folder, null)
      } else if (backups.view?.config) backups.view = await backend.backupsTurnOff()
      step = 'encrypt'
    } catch (err) {
      const e = err as { kind?: string; message?: string }
      error = backupError(e?.kind, e?.message ?? String(err))
    } finally {
      busy = false
    }
  }
</script>

{#if step === 'password'}
  <Setup
    encrypt
    {done}
    cancel={() => (encrypted ? done() : (step = 'encrypt'))}
    onencrypted={() => (encrypted = true)}
  />
{:else}
  <main class="screen">
    <div class="card">
      <span class="logo"><Knot size={48} /></span>
      {#if step === 'backups'}
        <h1>{t('welcome.backupsTitle')}</h1>
        <p class="muted">{t('welcome.backupsHelp')}</p>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <div class="actions">
          <button class="btn" type="button" disabled={busy} onclick={() => answerBackups(false)}
            >{t('common.no')}</button
          >
          <button
            class="btn primary"
            type="button"
            disabled={busy}
            onclick={() => answerBackups(true)}>{t('common.yes')}</button
          >
        </div>
      {:else}
        <h1>{t('welcome.encryptTitle')}</h1>
        <p class="muted">{t('welcome.encryptHelp')}</p>
        <div class="actions">
          <button class="btn" type="button" onclick={done}>{t('common.no')}</button>
          <button class="btn primary" type="button" onclick={() => (step = 'password')}
            >{t('common.yes')}</button
          >
        </div>
      {/if}
    </div>
  </main>
{/if}

<style>
  /* as the password steps that may follow */
  .screen {
    height: 100%;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    overflow: auto;
  }
  .card {
    width: min(420px, 100%);
    display: grid;
    gap: var(--space-4);
    padding: var(--space-6);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
    text-align: center;
  }
  .logo {
    display: grid;
    justify-self: center;
  }
  h1 {
    margin: 0;
    font-size: var(--title-screen);
  }
  p {
    margin: 0;
  }
  .error {
    color: var(--color-danger);
  }
  .actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2);
  }
  .actions .btn {
    height: 48px;
    justify-content: center;
    font-size: 16px;
  }
  /* phones: the card becomes the screen, no frame inside a frame */
  @media (max-width: 520px) {
    .screen {
      padding: var(--space-3);
    }
    .card {
      padding: var(--space-4) var(--space-2);
      background: none;
      border: 0;
    }
  }
</style>
