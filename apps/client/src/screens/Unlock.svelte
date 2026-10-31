<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The lock screen. One action, the way the vault opens day to day:
     the fingerprint (asked right away), Windows Hello (its button), the PIN pad, or
     the password, which always works and is one tap away from the others.
     An old vault this device opened alone only comes here when its key is
     gone. The rest (recovery key, starting again) waits behind "Trouble
     opening?". The knot ties itself as the notes lock and lets go as they
     open. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import { brand } from '@app/brand'
  import { onMount } from 'svelte'
  import { onBack } from '../lib/back'
  import { backend, type VaultStatus } from '../lib/backend'
  import { asksOnItsOwn, biometricMethod } from '../lib/biometric'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import Knot from '../lib/Knot.svelte'
  import PinPad from '../lib/PinPad.svelte'
  import { isBackendError } from '../lib/workspace.svelte'
  import ResetDialog from '../settings/ResetDialog.svelte'

  let { done, status }: { done: () => void; status: VaultStatus } = $props()

  let password = $state('')
  let busy = $state(false)
  let error = $state('')
  let resetting = $state(false)
  let trouble = $state(false)
  /** Opened: the knot lets go, then the notes come back. */
  let opening = $state(false)
  /** Hello or the fingerprint failed for good: the password, when there is one. */
  let stale = $state(false)
  /** The person chose the password over Hello, the fingerprint or the PIN. */
  let typing = $state(false)
  /** The PIN ran out of tries: the password from now on. */
  let pinGone = $state(false)
  /** Bumped on a wrong PIN, the dots shake. */
  let wrongPin = $state(0)

  const plain = $derived(status.way === 'plain')
  const guarded = $derived(status.way === 'guarded' && !stale)
  const pin = $derived(status.way === 'pin' && !pinGone)
  // a vault opened by this device alone whose key is gone: no password to type
  const keyGone = $derived(status.way === 'device' && !status.hasPassword)
  const quick = $derived((guarded || pin) && !typing)
  const showPassword = $derived(status.hasPassword && !quick)
  const method = $derived(biometricMethod())

  const kind = (err: unknown) => (isBackendError(err) ? err.kind : 'unknown')

  function opened() {
    opening = true
  }

  async function attempt(action: () => Promise<void>) {
    busy = true
    error = ''
    try {
      await action()
      opened()
    } catch (err) {
      const k = kind(err)
      if (k === 'cancelled') return
      error = t(`errors.${k}`)
      throw err
    } finally {
      busy = false
    }
  }

  async function retry() {
    await attempt(() => backend.vaultUnlock(null)).catch((err) => {
      // without a password no key is wrong: the file is damaged
      if (kind(err) === 'wrongKey') error = t('errors.corrupt')
      else if (kind(err) === 'notRemembered') error = t('unlock.keyGone')
    })
  }

  async function unlockBiometric() {
    await attempt(() => backend.vaultUnlockBiometric()).catch((err) => {
      if (kind(err) === 'notRemembered') {
        // the sealed key will never open again: the password, or recovery
        stale = true
        error = sentence(t('unlock.biometricChanged', { method }))
      }
    })
  }

  async function unlockPin(value: string) {
    await attempt(() => backend.vaultUnlockPin(value)).catch(async (err) => {
      if (kind(err) === 'wrongPassword') {
        wrongPin++
        const left = (await backend.vaultStatus()).pinTriesLeft ?? 0
        error = t('pin.wrong', { count: left })
      } else if (kind(err) === 'notRemembered') {
        pinGone = true
        error = t('pin.gone')
      }
    })
  }

  /** The method name may open the message, as in "fingerprint or face changed". */
  const sentence = (text: string) => text.charAt(0).toUpperCase() + text.slice(1)

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    if (!password) return
    await attempt(() => backend.vaultUnlock(password))
      .then(() => (password = ''))
      .catch(() => undefined)
  }

  // the recovery code
  let recovering = $state(false)
  $effect(() => {
    if (recovering) return onBack(() => ((recovering = false), (error = '')))
  })
  let code = $state('')
  let newPassword = $state('')
  let newConfirm = $state('')
  const MIN_LENGTH = 8
  const recoverProblem = $derived(
    newPassword.length < MIN_LENGTH
      ? t('setup.tooShort', { count: MIN_LENGTH })
      : newPassword !== newConfirm
        ? t('setup.mismatch')
        : '',
  )

  async function recover(event: SubmitEvent) {
    event.preventDefault()
    if (recoverProblem) return
    await attempt(() => backend.vaultRecover(code, newPassword)).catch((err) => {
      if (kind(err) === 'wrongPassword' || kind(err) === 'invalid') error = t('recovery.wrongKey')
    })
  }

  // ask right away on phones, the way they unlock themselves; Windows Hello
  // waits for its button (asksOnItsOwn)
  onMount(() => {
    if (plain || (status.way === 'device' && !status.hasPassword)) void retry()
    else if (guarded && asksOnItsOwn()) void unlockBiometric()
  })
</script>

<main class="screen">
  <div class="card" class:opening class:compact={pin && !typing && !recovering}>
    <div class="mark">
      <Knot
        motion={opening ? 'untie' : 'tie'}
        size={pin && !typing && !recovering ? 64 : 92}
        onend={() => opening && done()}
      />
    </div>

    {#if recovering}
      <form class="body" onsubmit={recover}>
        <h1>{t('recovery.forgotTitle')}</h1>
        <p class="muted">{t('recovery.forgotHelp')}</p>
        <label>
          {t('recovery.keyLabel')}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="field code"
            autocomplete="off"
            spellcheck="false"
            autocapitalize="characters"
            autofocus
            bind:value={code}
          />
        </label>
        <input type="text" autocomplete="username" value={brand.displayName} hidden readonly />
        <label>
          {t('settings.newPassword')}
          <PasswordField class="field" autocomplete="new-password" bind:value={newPassword} />
        </label>
        <label>
          {t('setup.confirm')}
          <PasswordField
            eye={false}
            class="field"
            autocomplete="new-password"
            bind:value={newConfirm}
          />
        </label>
        {#if newPassword && (newConfirm || newPassword.length < MIN_LENGTH) && recoverProblem}<p
            class="error"
          >
            {recoverProblem}
          </p>{/if}
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <div class="actions">
          <button class="btn" type="button" onclick={() => ((recovering = false), (error = ''))}
            >{t('common.back')}</button
          >
          <button class="btn primary" type="submit" disabled={busy || !code || !!recoverProblem}
            >{t('unlock.open')}</button
          >
        </div>
      </form>
    {:else}
      <div class="body">
        <h1>{plain || keyGone ? t('unlock.failedTitle') : t('unlock.title')}</h1>
        <p class="muted subtitle">
          {#if plain}{t('unlock.failedHelp')}
          {:else if keyGone}{t('unlock.keyGoneHelp')}
          {:else if guarded && !typing}{t('unlock.guardedHelp', { method })}
          {:else if pin && !typing}{t('pin.help')}
          {:else}{t('unlock.passwordHelp')}{/if}
        </p>

        {#if guarded && !typing}
          <button class="btn primary big" type="button" disabled={busy} onclick={unlockBiometric}>
            <Icon name="fingerprint" size={20} />{t('unlock.biometric', { method })}
          </button>
        {:else if pin && !typing}
          <PinPad label={t('pin.label')} disabled={busy} shake={wrongPin} onpin={unlockPin} />
        {:else if showPassword}
          <form class="password" onsubmit={submit}>
            <!-- password managers and Chromium expect a username next to a password -->
            <input type="text" autocomplete="username" value={brand.displayName} hidden readonly />
            <PasswordField
              class="field"
              autocomplete="current-password"
              aria-label={t('unlock.password')}
              placeholder={t('unlock.password')}
              autofocus
              bind:value={password}
            />
            <button
              class="btn primary go"
              type="submit"
              disabled={busy || !password}
              aria-label={t('unlock.submit')}
              title={t('unlock.submit')}><Icon name="right" size={18} /></button
            >
          </form>
        {:else if plain || keyGone}
          <button class="btn primary big" type="button" disabled={busy} onclick={retry}
            >{t('unlock.retry')}</button
          >
        {/if}

        {#if error}<p class="error" role="alert">{error}</p>{/if}

        <!-- the password always works, the quick way is one tap back -->
        {#if quick && status.hasPassword}
          <button class="link" type="button" onclick={() => ((typing = true), (error = ''))}
            >{t('unlock.usePassword')}</button
          >
        {:else if typing && guarded}
          <button class="link" type="button" onclick={() => ((typing = false), (error = ''))}
            >{t('unlock.biometric', { method })}</button
          >
        {:else if typing && pin}
          <button class="link" type="button" onclick={() => ((typing = false), (error = ''))}
            >{t('pin.use')}</button
          >
        {/if}
      </div>

      <div class="trouble">
        <button class="link quiet" aria-expanded={trouble} onclick={() => (trouble = !trouble)}
          >{t('unlock.trouble')}</button
        >
        {#if trouble}
          <div class="trouble-box">
            {#if status.recovery}
              <button class="btn" onclick={() => ((recovering = true), (error = ''))}
                ><Icon name="key" size={16} />{t('unlock.useRecovery')}</button
              >
            {/if}
            <p class="muted small">{t('unlock.syncedHelp')}</p>
            <button class="btn danger" onclick={() => (resetting = true)}
              >{t('unlock.startAgain')}</button
            >
          </div>
        {/if}
      </div>
    {/if}
  </div>
</main>

{#if resetting}
  <ResetDialog close={() => (resetting = false)} />
{/if}

<style>
  .screen {
    height: 100%;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    overflow: auto;
    background:
      radial-gradient(
        120% 70% at 50% 0%,
        color-mix(in srgb, var(--color-accent) 9%, transparent),
        transparent 60%
      ),
      var(--color-bg);
  }
  .card {
    width: min(380px, 100%);
    display: grid;
    justify-items: center;
    gap: var(--space-5);
    padding: var(--space-6) var(--space-5) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
    box-shadow: 0 18px 50px rgb(0 0 0 / 0.28);
    animation: card-in 260ms ease-out both;
  }
  /* the PIN pad is tall: a smaller knot and tighter spacing keep it all in
     a normal window */
  .card.compact {
    gap: var(--space-3);
    padding-top: var(--space-5);
  }
  .card.compact .mark {
    width: 88px;
    height: 88px;
  }
  .card.compact .trouble {
    padding-top: var(--space-2);
  }
  .card.opening {
    animation: card-out 300ms ease-in 120ms forwards;
  }
  @keyframes card-in {
    from {
      opacity: 0;
      transform: translateY(6px) scale(0.99);
    }
  }
  @keyframes card-out {
    to {
      opacity: 0;
      transform: scale(0.98);
    }
  }
  .mark {
    /* the knot cuts its crossing with the colour behind it */
    --knot-behind: color-mix(in srgb, var(--color-text) 6%, var(--color-surface));
    display: grid;
    place-items: center;
    width: 120px;
    height: 120px;
    border-radius: 50%;
    background: var(--knot-behind);
  }
  .body,
  .password,
  form.body {
    width: 100%;
    display: grid;
    gap: var(--space-3);
  }
  h1 {
    margin: 0;
    font-size: 21px;
    text-align: center;
  }
  .subtitle {
    margin: 0 0 var(--space-1);
    text-align: center;
  }
  p {
    margin: 0;
  }
  .small {
    font-size: 13px;
  }
  .big {
    height: 50px;
    justify-content: center;
    gap: var(--space-2);
    font-size: 16px;
  }
  .password {
    grid-template-columns: 1fr auto;
    gap: var(--space-2);
  }
  .password :global(input.field) {
    height: 46px;
    font-size: 16px;
  }
  .go {
    width: 46px;
    height: 46px;
    justify-content: center;
    padding: 0;
  }
  label {
    display: grid;
    gap: var(--space-1);
  }
  .error {
    color: var(--color-danger);
    text-align: center;
  }
  .code {
    font-family: var(--font-mono);
    text-transform: uppercase;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .link {
    justify-self: center;
    /* a finger sized target around the words */
    padding: var(--space-2);
    border: 0;
    background: none;
    color: var(--color-accent-text);
    cursor: pointer;
    font: inherit;
  }
  .link:hover {
    text-decoration: underline;
  }
  .quiet {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .trouble {
    width: 100%;
    display: grid;
    justify-items: center;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
  }
  .trouble-box {
    width: 100%;
    display: grid;
    gap: var(--space-2);
    text-align: center;
  }
  .trouble-box .btn {
    justify-content: center;
    gap: var(--space-2);
  }
  /* phones: the card becomes the screen, no frame inside a frame */
  @media (max-width: 520px) {
    .card {
      padding: var(--space-5) var(--space-2) var(--space-3);
      background: none;
      border: 0;
      box-shadow: none;
    }
    .mark {
      --knot-behind: color-mix(in srgb, var(--color-text) 6%, var(--color-bg));
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .card,
    .card.opening {
      animation-duration: 1ms;
      animation-delay: 0ms;
    }
  }
</style>
