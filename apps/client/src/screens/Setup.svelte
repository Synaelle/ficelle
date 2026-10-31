<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- First launch. First the kind of notes: encrypted (recommended), or
     standard, open to anyone using the device, which ends the setup there.
     Encrypted, a password next: it always opens the notes, and it is the
     way back when the quicker way is gone. Then how they open day to day:
     Windows Hello or a fingerprint, a PIN typed here, or the password each
     time. An old vault that opened without asking goes through the password
     steps once open, `upgrade` set. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import { brand } from '@app/brand'
  import { onMount } from 'svelte'
  import logo from '../../../../packages/ui/brand/app-icon.svg?url'
  import { onBack } from '../lib/back'
  import { backend, PIN_LENGTH, type VaultWay } from '../lib/backend'
  import { biometricMethod } from '../lib/biometric'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import PinPad from '../lib/PinPad.svelte'
  import { isBackendError } from '../lib/workspace.svelte'
  import RecoveryKey from './RecoveryKey.svelte'

  let {
    done,
    upgrade,
  }: {
    done: () => void
    /** An old vault that opened without asking, open already. */
    upgrade?: boolean
  } = $props()

  type Step = 'kind' | 'password' | 'quick' | 'pin' | 'recovery'
  // svelte-ignore state_referenced_locally
  let step: Step = $state(upgrade ? 'password' : 'kind')
  /** Encrypted notes, or standard ones open to anyone using the device. */
  let encrypted = $state(true)
  $effect(() => {
    if (step === 'password' && !upgrade) return onBack(() => (step = 'kind'))
    if (step === 'quick') return onBack(() => (step = 'password'))
    if (step === 'pin') return onBack(() => ((step = 'quick'), (firstPin = '')))
  })

  let password = $state('')
  let confirm = $state('')
  /** A recovery key is the way back when the password is forgotten. */
  let wantRecovery = $state(true)
  let recoveryCode = $state('')
  let biometricAvailable = $state(false)
  let deviceAvailable = $state(true)
  let way: VaultWay = $state('password')
  /** The PIN typed once, waiting for the second time. */
  let firstPin = $state('')
  let pinShake = $state(0)
  let busy = $state(false)
  let error = $state('')
  const method = $derived(biometricMethod())

  onMount(async () => {
    const status = await backend.vaultStatus()
    biometricAvailable = status.biometricAvailable
    deviceAvailable = status.deviceAvailable
    way = biometricAvailable ? 'guarded' : deviceAvailable ? 'pin' : 'password'
  })

  const MIN_LENGTH = 8
  /** Next stays off while this says something; it shows only once there is
   * something to say about what was typed, the second field included. */
  const problem = $derived(
    password.length < MIN_LENGTH
      ? t('setup.tooShort', { count: MIN_LENGTH })
      : password !== confirm
        ? t('setup.mismatch')
        : '',
  )

  const shownProblem = $derived(
    !password
      ? ''
      : password.length < MIN_LENGTH
        ? t('setup.tooShort', { count: MIN_LENGTH })
        : confirm && password !== confirm
          ? t('setup.mismatch')
          : '',
  )

  function submitKind(event: SubmitEvent) {
    event.preventDefault()
    error = ''
    if (encrypted) step = 'password'
    else void plain()
  }

  function submitPassword(event: SubmitEvent) {
    event.preventDefault()
    if (problem) return
    error = ''
    step = 'quick'
  }

  function chooseWay(event: SubmitEvent) {
    event.preventDefault()
    error = ''
    if (way === 'pin') step = 'pin'
    else void finish(null)
  }

  function typedPin(pin: string) {
    error = ''
    if (!firstPin) {
      firstPin = pin
      return
    }
    if (pin !== firstPin) {
      firstPin = ''
      pinShake++
      error = t('pin.mismatch')
      return
    }
    void finish(pin)
  }

  async function finish(pin: string | null) {
    busy = true
    error = ''
    try {
      if (upgrade) await backend.vaultSetWay(way, null, password, pin)
      else await backend.vaultCreate(way, password, pin)
      if (wantRecovery) {
        recoveryCode = await backend.vaultCreateRecovery(password)
        step = 'recovery'
      } else {
        done()
      }
    } catch (err) {
      firstPin = ''
      // a cancelled Windows Hello prompt leaves the choice on screen
      if (!isBackendError(err) || err.kind !== 'cancelled')
        error = t(`errors.${isBackendError(err) ? err.kind : 'unknown'}`)
    } finally {
      busy = false
    }
  }

  /** No password, no encryption: the notes open as soon as the app does. */
  async function plain() {
    busy = true
    error = ''
    try {
      await backend.vaultCreate('plain', '', null)
      done()
    } catch (err) {
      error = t(`errors.${isBackendError(err) ? err.kind : 'unknown'}`)
    } finally {
      busy = false
    }
  }

  interface Choice {
    way: VaultWay
    icon: IconName
    title: string
    help: string
  }
  const choices = $derived.by(() => {
    const out: Choice[] = []
    if (biometricAvailable)
      out.push({
        way: 'guarded',
        icon: 'fingerprint',
        title: t('setup.quickBiometric', { method }),
        help: t('setup.quickBiometricHelp'),
      })
    if (deviceAvailable)
      out.push({
        way: 'pin',
        icon: 'key',
        title: t('setup.quickPin', { count: PIN_LENGTH }),
        help: t('setup.quickPinHelp'),
      })
    out.push({
      way: 'password',
      icon: 'lock',
      title: t('setup.quickPassword'),
      help: t('setup.quickPasswordHelp'),
    })
    return out
  })
</script>

<main class="screen">
  <div class="card">
    {#if step === 'kind'}
      <form onsubmit={submitKind}>
        <img class="logo" src={logo} alt="" />
        <h1>{t('setup.title', { appName: brand.displayName })}</h1>
        <p class="muted center">{t('setup.welcome')}</p>
        <div class="choices" role="radiogroup" aria-label={t('setup.kindTitle')}>
          <label class="choice" class:chosen={encrypted}>
            <input type="radio" name="kind" value={true} bind:group={encrypted} />
            <span class="tile"><Icon name="lock" size={18} /></span>
            <span class="words">
              <strong>{t('setup.kindEncrypted')}</strong>
              <span class="muted small">{t('setup.kindEncryptedHelp')}</span>
            </span>
          </label>
          <label class="choice" class:chosen={!encrypted}>
            <input type="radio" name="kind" value={false} bind:group={encrypted} />
            <span class="tile"><Icon name="eye" size={18} /></span>
            <span class="words">
              <strong>{t('setup.kindStandard')}</strong>
              <span class="muted small">{t('setup.plainHelp')}</span>
            </span>
          </label>
        </div>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <button class="btn primary big" type="submit" disabled={busy}
          >{busy ? t('setup.working') : encrypted ? t('setup.next') : t('setup.start')}</button
        >
      </form>
    {:else if step === 'password'}
      <form onsubmit={submitPassword}>
        {#if upgrade}
          <img class="logo" src={logo} alt="" />
          <h1>{t('setup.upgradeTitle')}</h1>
          <p class="muted center">{t('setup.upgradeHelp')}</p>
        {:else}
          <h1>{t('setup.passwordTitle')}</h1>
        {/if}
        <p class="muted small center">{t('setup.passwordWhy')}</p>
        <!-- password managers and Chromium expect a username next to a password -->
        <input type="text" autocomplete="username" value={brand.displayName} hidden readonly />
        <PasswordField
          class="field"
          autocomplete="new-password"
          aria-label={t('setup.password')}
          placeholder={t('setup.password')}
          autofocus
          bind:value={password}
        />
        <PasswordField
          eye={false}
          class="field"
          autocomplete="new-password"
          aria-label={t('setup.confirm')}
          placeholder={t('setup.confirm')}
          bind:value={confirm}
        />
        {#if shownProblem}<p class="error">{shownProblem}</p>{/if}
        <label class="option">
          <input type="checkbox" bind:checked={wantRecovery} />
          {t('recovery.create')}
        </label>
        <p class="small" class:warning={!wantRecovery} class:muted={wantRecovery}>
          {wantRecovery ? t('recovery.createHelp') : t('setup.noRecovery')}
        </p>
        {#if upgrade}
          <button class="btn primary big" type="submit" disabled={!!problem}
            >{t('setup.next')}</button
          >
        {:else}
          <div class="actions">
            <button class="btn" type="button" onclick={() => (step = 'kind')}
              >{t('common.back')}</button
            >
            <button class="btn primary" type="submit" disabled={!!problem}>{t('setup.next')}</button
            >
          </div>
        {/if}
      </form>
    {:else if step === 'quick'}
      <form onsubmit={chooseWay}>
        <h1>{t('setup.quickTitle')}</h1>
        <p class="muted center">{t('setup.quickHelp')}</p>
        <div class="choices" role="radiogroup" aria-label={t('setup.quickTitle')}>
          {#each choices as choice (choice.way)}
            <label class="choice" class:chosen={way === choice.way}>
              <input type="radio" name="way" value={choice.way} bind:group={way} />
              <span class="tile"><Icon name={choice.icon} size={18} /></span>
              <span class="words">
                <strong>{choice.title}</strong>
                <span class="muted small">{choice.help}</span>
              </span>
            </label>
          {/each}
        </div>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <div class="actions">
          <button class="btn" type="button" onclick={() => (step = 'password')}
            >{t('common.back')}</button
          >
          <button class="btn primary" type="submit" disabled={busy}
            >{busy
              ? t('setup.working')
              : way === 'pin'
                ? t('setup.next')
                : t('setup.start')}</button
          >
        </div>
      </form>
    {:else if step === 'pin'}
      <div class="pin-step">
        <h1>{firstPin ? t('pin.confirm') : t('pin.choose', { count: PIN_LENGTH })}</h1>
        <p class="muted center small">{t('setup.quickPinHelp')}</p>
        <PinPad
          label={firstPin ? t('pin.confirm') : t('pin.choose', { count: PIN_LENGTH })}
          disabled={busy}
          shake={pinShake}
          onpin={typedPin}
        />
        {#if busy}<p class="muted center">{t('setup.working')}</p>{/if}
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <button
          class="btn"
          type="button"
          disabled={busy}
          onclick={() => ((step = 'quick'), (firstPin = ''), (error = ''))}
          >{t('common.back')}</button
        >
      </div>
    {:else}
      <RecoveryKey code={recoveryCode} {done} level="h1" />
    {/if}
  </div>
</main>

<style>
  .screen {
    height: 100%;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    overflow: auto;
  }
  .card,
  form,
  .pin-step {
    width: min(420px, 100%);
    display: grid;
    gap: var(--space-4);
  }
  .card {
    padding: var(--space-6);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  .pin-step {
    justify-items: center;
  }
  .logo {
    width: 72px;
    height: 72px;
    justify-self: center;
  }
  h1 {
    margin: 0;
    font-size: 22px;
    text-align: center;
  }
  p {
    margin: 0;
  }
  .center {
    text-align: center;
  }
  .small {
    font-size: 13px;
  }
  .big {
    height: 48px;
    justify-content: center;
    font-size: 16px;
  }
  .choices {
    display: grid;
    gap: var(--space-2);
  }
  .choice {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    cursor: pointer;
  }
  .choice.chosen {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 8%, transparent);
  }
  .choice input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }
  .choice:has(input:focus-visible) {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .tile {
    display: grid;
    place-items: center;
    flex: none;
    width: 36px;
    height: 36px;
    border-radius: 10px;
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent-text);
  }
  .words {
    display: grid;
    gap: 2px;
  }
  .option {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 32px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .warning {
    color: var(--color-warning);
  }
  .error {
    color: var(--color-danger);
    text-align: center;
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
