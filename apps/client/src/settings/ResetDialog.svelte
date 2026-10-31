<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Deleting everything on this device. The button waits a few seconds
     before it can be pressed, so the choice is read, not clicked through.
     Used by the settings and by the unlock screen, where notes that no
     longer open can still be started over. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { backend } from '../lib/backend'
  import Icon from '../lib/icons/Icon.svelte'
  import { t } from '../lib/i18n.svelte'
  import { modal } from '../lib/modal'
  import { isBackendError } from '../lib/workspace.svelte'

  let { close }: { close: () => void } = $props()

  const WAIT_SECONDS = 10

  let left = $state(WAIT_SECONDS)
  let busy = $state(false)
  let error: string | null = $state(null)

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
      await backend.vaultReset()
      try {
        // this device's display choices go too: a fresh start
        localStorage.clear()
      } catch {
        // storage can be unavailable, the notes are gone all the same
      }
      location.reload()
    } catch (err) {
      error = isBackendError(err) ? t(`errors.${err.kind}`) : t('errors.unknown')
      busy = false
    }
  }

  const closeIfIdle = () => !busy && close()
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
      <h2 id="reset-title">{t('reset.title')}</h2>
      <button class="icon-btn" aria-label={t('common.close')} disabled={busy} onclick={close}
        ><Icon name="close" /></button
      >
    </header>
    <div id="reset-body">
      <p>{t('reset.body')}</p>
      <p class="muted">{t('reset.server')}</p>
      <p class="strong">{t('reset.final')}</p>
    </div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions">
      <button class="btn" disabled={busy} onclick={close}>{t('common.cancel')}</button>
      <button class="btn danger" disabled={left > 0 || busy} onclick={reset}>
        {left > 0 ? t('reset.wait', { count: left }) : t('reset.confirm')}
      </button>
    </div>
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
    border: 1px solid var(--color-border);
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
    font-size: 16px;
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
