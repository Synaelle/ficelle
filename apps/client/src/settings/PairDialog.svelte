<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Pairing with another device: `show` puts a code on this one for the
     other to type, `type` takes the code shown on the other. On a phone
     the dialog fills the screen. On Android the app first says why it asks
     for Nearby devices, then the system asks. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { t } from '../lib/i18n.svelte'
  import { isLocalError, localSync } from '../lib/localSync.svelte'
  import { modal } from '../lib/modal'
  import { WINDOWS } from '../lib/platform'

  let { mode, close }: { mode: 'show' | 'type'; close: () => void } = $props()

  /** `asking`: the explanation before the system's prompt. */
  let step: 'checking' | 'asking' | 'denied' | 'ready' = $state('checking')
  let code = $state('')
  let address = $state('')
  let byAddress = $state(false)
  let busy = $state(false)
  let error = $state('')
  /** The device paired from this side, by typing. */
  let typedPaired: string | null = $state(null)
  let copied = $state(false)
  let addressOpen = $state(false)

  const pairing = $derived(localSync.status.pairing)
  const pairedName = $derived(mode === 'show' ? (pairing?.paired ?? null) : typedPaired)
  const pairedDevice = $derived(
    pairedName ? localSync.status.devices.find((d) => d.name === pairedName) : undefined,
  )
  const digits = $derived(code.replace(/\D/g, '').slice(0, 6))

  const grouped = (c: string) => (c.length > 3 ? `${c.slice(0, 3)} ${c.slice(3)}` : c)

  function explain(err: unknown) {
    const kind = isLocalError(err) ? err.kind : ''
    return ['wrongCode', 'notFound', 'serverLinked', 'locked'].includes(kind)
      ? t(`local.error.${kind}`)
      : t('local.error.other')
  }

  async function begin() {
    if (mode === 'show') {
      try {
        await localSync.pairStart()
      } catch (err) {
        error = explain(err)
      }
    }
    step = 'ready'
  }

  onMount(() => {
    void (async () => {
      const state = await localSync.permission(false).catch(() => 'granted')
      if (state === 'granted') await begin()
      else step = 'asking'
    })()
    return () => {
      if (mode === 'show' && !localSync.status.pairing?.paired) void localSync.pairCancel()
    }
  })

  async function allow() {
    const state = await localSync.permission(true).catch(() => 'denied')
    if (state === 'granted') await begin()
    else step = 'denied'
  }

  function submit(event: SubmitEvent) {
    event.preventDefault()
    if (digits.length !== 6 || busy) return
    busy = true
    error = ''
    localSync
      .pairCode(digits, byAddress ? address.trim() : null)
      .then((name) => (typedPaired = name))
      .catch((err) => (error = explain(err)))
      .finally(() => (busy = false))
  }

  async function copy() {
    if (!pairing?.address) return
    await navigator.clipboard.writeText(pairing.address).catch(() => {})
    copied = true
  }

  const minutes = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
  const closeIfIdle = () => !busy && close()
</script>

<div
  class="backdrop"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && closeIfIdle()}
>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="pair-title"
    tabindex="-1"
    data-autofocus
    use:modal={{ onclose: closeIfIdle }}
  >
    <header>
      <h2 id="pair-title">
        {pairedName
          ? t('local.pairing.paired', { name: pairedName })
          : mode === 'show'
            ? t('local.pairing.title')
            : t('local.code.title')}
      </h2>
      <button class="icon-btn" aria-label={t('common.close')} disabled={busy} onclick={close}
        ><Icon name="close" /></button
      >
    </header>

    {#if step === 'asking' || step === 'denied'}
      <h3 class="ask">{t('local.permission.title')}</h3>
      <p class="muted">{t('local.permission.body')}</p>
      <p class="muted">{t('local.permission.promise')}</p>
      {#if step === 'denied'}<p class="error" role="alert">{t('local.permission.denied')}</p>{/if}
      <div class="actions">
        <button class="btn" onclick={close}>{t('local.permission.notNow')}</button>
        <button class="btn primary" onclick={allow}>{t('local.permission.continue')}</button>
      </div>
    {:else if step === 'checking'}
      <p class="muted">{t('local.pairing.starting')}</p>
    {:else if pairedName}
      <div class="box">
        <span class="line">
          <Icon name={pairedDevice?.state === 'synced' ? 'check' : 'syncing'} size={16} />
          {pairedDevice?.state === 'synced'
            ? t('local.state.synced')
            : t('local.pairing.firstSync')}
        </span>
        <span class="muted small">{t('local.pairing.goesOn')}</span>
      </div>
      <p class="muted small">{t('local.pairing.pairedHelp', { name: pairedName })}</p>
      <div class="actions">
        <button class="btn primary" onclick={close}>{t('local.pairing.done')}</button>
      </div>
    {:else if mode === 'show'}
      {#if pairing && !pairing.failed && pairing.code}
        <p>{t('local.pairing.typeOn')}</p>
        <p class="code">{grouped(pairing.code)}</p>
        <p class="line muted">
          <Icon name="syncing" size={16} />
          <span class="grow">{t('local.pairing.waiting')}</span>
          <span class="mono small"
            >{t('local.pairing.left', { time: minutes(pairing.secondsLeft) })}</span
          >
        </p>
        <p class="muted small">
          {t('local.pairing.sameNetwork')}
          {#if WINDOWS}{t('local.pairing.windowsFirewall')}{/if}
        </p>
        {#if pairing.address}
          <div class="fold">
            <button
              class="link"
              aria-expanded={addressOpen}
              onclick={() => (addressOpen = !addressOpen)}
            >
              <span class="chevron" class:open={addressOpen}><Icon name="down" size={16} /></span>
              {t('local.pairing.cantFind')}
            </button>
            {#if addressOpen}
              <p class="muted small">{t('local.pairing.cantFindHelp')}</p>
              <div class="address">
                <span class="mono grow">{pairing.address}</span>
                <button class="btn" onclick={copy}
                  >{copied ? t('local.pairing.copied') : t('local.pairing.copy')}</button
                >
              </div>
            {/if}
          </div>
        {/if}
      {:else if pairing?.failed}
        <p class="error" role="alert">{t(`local.pairing.${pairing.failed}`)}</p>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="actions">
        <button class="btn" onclick={() => void begin()}>{t('local.pairing.newCode')}</button>
        <button class="btn" onclick={close}>{t('common.cancel')}</button>
      </div>
    {:else}
      <form onsubmit={submit}>
        <label for="pair-code" class="label">{t('local.code.label')}</label>
        <p class="muted small">{t('local.code.where')}</p>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          id="pair-code"
          class="field code-field"
          inputmode="numeric"
          autocomplete="one-time-code"
          maxlength="7"
          autofocus
          aria-describedby="pair-code-help"
          value={grouped(digits)}
          oninput={(e) => (code = e.currentTarget.value)}
        />
        <p id="pair-code-help" class="muted small">{t('local.code.help')}</p>
        {#if byAddress}
          <input
            class="field"
            inputmode="url"
            placeholder={t('local.code.address')}
            aria-label={t('local.code.address')}
            bind:value={address}
          />
        {:else}
          <button class="link" type="button" onclick={() => (byAddress = true)}
            >{t('local.code.typeAddress')}</button
          >
        {/if}
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <div class="actions">
          <button class="btn" type="button" disabled={busy} onclick={close}
            >{t('common.cancel')}</button
          >
          <button
            class="btn primary"
            type="submit"
            disabled={busy || digits.length !== 6 || (byAddress && !address.trim())}
            >{busy ? t('local.code.pairing') : t('local.code.pair')}</button
          >
        </div>
      </form>
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
  /* the dialog itself takes the focus as it opens, its close button not */
  .dialog:focus {
    outline: none;
  }
  .dialog {
    width: min(460px, 100%);
    max-height: calc(100vh - 2 * var(--space-4));
    overflow-y: auto;
    display: grid;
    gap: var(--space-3);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  /* a phone: the pairing screens are pages of their own */
  @media (max-width: 600px) {
    .backdrop {
      padding: 0;
      place-items: stretch;
    }
    .dialog {
      width: 100%;
      max-height: none;
      align-content: start;
      border: 0;
      border-radius: 0;
    }
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
    overflow-wrap: anywhere;
  }
  h3.ask {
    margin: 0;
    font-size: 18px;
    line-height: 1.3;
  }
  p {
    margin: 0;
  }
  form {
    display: grid;
    gap: var(--space-3);
  }
  .label {
    font-size: 16px;
    font-weight: 700;
  }
  .muted {
    color: var(--color-text-muted);
  }
  .small {
    font-size: 13px;
  }
  .mono {
    font-family: var(--font-mono);
  }
  .grow {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .code {
    padding: var(--space-5) 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-bg);
    font-family: var(--font-mono);
    font-size: 44px;
    font-weight: 500;
    letter-spacing: 0.08em;
    text-align: center;
  }
  .code-field {
    height: 64px;
    font-family: var(--font-mono);
    font-size: 32px;
    letter-spacing: 0.12em;
    text-align: center;
  }
  .box {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface-2);
  }
  .fold {
    display: grid;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
  }
  .address {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface-2);
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-accent-text);
    font: inherit;
    cursor: pointer;
  }
  .fold .link {
    color: var(--color-text);
    font-weight: 600;
  }
  .chevron {
    display: grid;
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .chevron.open {
    transform: rotate(180deg);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .error {
    color: var(--color-danger);
  }
</style>
