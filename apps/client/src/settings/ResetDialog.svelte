<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Deleting everything on this device. The button waits a few seconds
     before it can be pressed, so the choice is read, not clicked through.
     Used by the settings and by the unlock screen, where notes that no
     longer open can still be started over: there the files are set aside
     in a folder beside them instead, and the dialog says which. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { backend } from '../lib/backend'
  import { shownError, type ShownError } from '../lib/errors'
  import ErrorText from '../lib/ErrorText.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { t } from '../lib/i18n.svelte'
  import { modal } from '../lib/modal'

  let { close, aside = false }: { close: () => void; aside?: boolean } = $props()

  // dev and preview builds are reset often while testing: a short pause
  const WAIT_SECONDS = __APP_DEV_TOOLS__ ? 2 : 10

  let left = $state(WAIT_SECONDS)
  let busy = $state(false)
  let error: ShownError | null = $state(null)
  /** Where the files were set aside, once they were. */
  let folder = $state('')

  onMount(() => {
    const timer = setInterval(() => {
      left = Math.max(0, left - 1)
      if (left === 0) clearInterval(timer)
    }, 1000)
    return () => clearInterval(timer)
  })

  async function reset() {
    if (left > 0 || busy) return
    busy = true
    error = null
    try {
      if (aside) {
        folder = await backend.vaultSetAside()
        busy = false
        return
      }
      await backend.vaultReset()
      try {
        // this device's display choices go too: a fresh start
        localStorage.clear()
      } catch {
        // storage can be unavailable, the notes are gone all the same
      }
      location.reload()
    } catch (err) {
      error = shownError(err, 'RESET')
      busy = false
    }
  }

  const closeIfIdle = () => !busy && !folder && close()
</script>

<div
  class="backdrop"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && closeIfIdle()}
>
  <div
    class="dialog"
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="reset-title"
    aria-describedby="reset-body"
    use:modal={{ onclose: closeIfIdle }}
  >
    <header>
      <h2 id="reset-title">{aside ? t('unlock.startAgain') : t('reset.title')}</h2>
      {#if !folder}
        <button class="icon-btn" aria-label={t('common.close')} disabled={busy} onclick={close}
          ><Icon name="close" /></button
        >
      {/if}
    </header>
    {#if folder}
      <div id="reset-body">
        <p role="status">{t('reset.asideDone')}</p>
        <p class="folder">{folder}</p>
      </div>
      <div class="actions">
        <button class="btn primary" onclick={() => location.reload()}>{t('common.ok')}</button>
      </div>
    {:else}
      <div id="reset-body">
        <p>{aside ? t('reset.asideBody') : t('reset.body')}</p>
        <p class="muted">{t('reset.server')}</p>
        {#if !aside}<p class="strong">{t('reset.final')}</p>{/if}
      </div>
      {#if error}<p class="error" role="alert"><ErrorText {error} /></p>{/if}
      <div class="actions">
        <button class="btn danger" disabled={left > 0 || busy} onclick={reset}>
          {#if aside}
            {left > 0 ? t('reset.asideWait', { count: left }) : t('reset.asideConfirm')}
          {:else}
            {left > 0 ? t('reset.wait', { count: left }) : t('reset.confirm')}
          {/if}
        </button>
        <button class="btn" disabled={busy} onclick={close}>{t('common.cancel')}</button>
      </div>
    {/if}
  </div>
</div>

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
    max-height: calc(100vh - 2 * var(--space-4));
    overflow-y: auto;
    display: grid;
    gap: var(--space-3);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-window);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: var(--title-dialog);
  }
  #reset-body {
    display: grid;
    gap: var(--space-2);
  }
  p {
    margin: 0;
  }
  .strong {
    font-weight: 600;
  }
  .folder {
    font-size: 13px;
    overflow-wrap: anywhere;
    user-select: text;
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
</style>
