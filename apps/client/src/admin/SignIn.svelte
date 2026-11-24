<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Sign-in to the admin page: a passkey first when the server and the
     browser have them, else a password and maybe a second factor. An
     end-to-end encrypted account signs in over OPAQUE, its password never
     leaves the page. On a server not set up yet it points to the setup link
     in the server log. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import { brand } from '@app/brand'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import {
    api,
    ApiError,
    keep,
    thisDevice,
    type AuthOptions,
    type PasskeyCeremony,
    type SecondFactorNeeded,
    type Session,
  } from './api.svelte'
  import { kindOf, signIn as e2eeSignIn, type AppKey } from './e2ee'
  import { errorText } from './format'
  import { passkeysSupported, usePasskey } from './webauthn'

  let {
    setup,
    options = null,
    initialProblem = '',
    heading = '',
    forApp,
    done,
  }: {
    setup: boolean
    options?: AuthOptions | null
    initialProblem?: string
    /** In place of the admin page's title, for connecting an app. */
    heading?: string
    /** Signing an app in: an encrypted account's export key is sealed to it. */
    forApp?: AppKey
    /** Signed in, with the export key sealed for `forApp`. */
    done: (sealed?: string) => void
  } = $props()

  const mode = $derived(setup ? 'setup' : 'signIn')

  let username = $state('')
  let password = $state('')
  let busy = $state(false)
  // svelte-ignore state_referenced_locally
  let problem = $state(initialProblem)

  /** Set once the password was right and a second factor is due. */
  let second: SecondFactorNeeded | null = $state(null)
  let factorCode = $state('')
  let recovery = $state(false)
  /** A code by email was asked for: its field shows. */
  let byEmail = $state(false)

  async function sendEmailCode() {
    const challenge = second?.challenge
    if (!challenge) return
    await attempt(async () => {
      await api('POST', '/auth/second-factor/email', { challenge })
      byEmail = true
      recovery = false
      factorCode = ''
    })
  }

  /** A passkey first. Not when signing an app in on a server that may hold
   * encrypted accounts: their notes open with the password only, the
   * passkey is still there as the second step. */
  const canPasskey = $derived(
    !!options?.passkeys && passkeysSupported() && !(forApp && options.accountKinds !== 'standard'),
  )

  async function attempt(action: () => Promise<void>) {
    problem = ''
    busy = true
    try {
      await action()
    } catch (err) {
      if (err instanceof DOMException) problem = t('admin.signIn.passkeyCancelled')
      else if (err instanceof ApiError) problem = errorText(err)
      else {
        // not the server: the browser, or an extension standing in for it,
        // failed with the passkey. Its own words say more than a guess.
        console.error(err)
        problem = t('admin.signIn.passkeyBrowser', {
          why: err instanceof Error ? err.message : String(err),
        })
      }
    } finally {
      busy = false
    }
  }

  /** The export key sealed at the password step, handed on once the second
   * step is through too. */
  let sealed: string | undefined

  function signedIn(session: Session) {
    keep(session)
    done(sealed)
  }

  /** Without a username the browser offers the passkeys it holds for this
   * server. A typed one also reaches passkeys made before they were kept on
   * the authenticator. */
  async function signInWithPasskey() {
    await attempt(async () => {
      const ceremony = await api<PasskeyCeremony>('POST', '/auth/passkey/start', { username })
      const credential = await usePasskey(ceremony.options)
      signedIn(
        await api<Session>('POST', '/auth/passkey/finish', {
          id: ceremony.id,
          credential,
          device: thisDevice(),
        }),
      )
    })
  }

  async function sendFactor(method: 'totp' | 'recoveryCode' | 'passkey' | 'email') {
    const challenge = second?.challenge
    if (!challenge) return
    await attempt(async () => {
      const passkey =
        method === 'passkey' && second?.passkey ? await usePasskey(second.passkey) : undefined
      signedIn(
        await api<Session>('POST', '/auth/second-factor', {
          challenge,
          method,
          code: method === 'passkey' ? undefined : factorCode,
          passkey,
        }),
      )
    })
  }

  /** The forgotten password panel, and whether its mail was asked for. */
  let forgot = $state(false)
  let resetAsked = $state(false)

  async function askReset(event: SubmitEvent) {
    event.preventDefault()
    await attempt(async () => {
      await api('POST', '/auth/reset/request', { username })
      resetAsked = true
    })
  }

  /** The second step's field takes the focus as it appears. */
  const focus = (field: HTMLElement) => field.focus()

  function startOver() {
    second = null
    sealed = undefined
    factorCode = ''
    recovery = false
    byEmail = false
    problem = ''
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    problem = ''
    if (second) {
      await sendFactor(recovery ? 'recoveryCode' : byEmail ? 'email' : 'totp')
      return
    }
    await attempt(async () => {
      const device = thisDevice()
      {
        let answer: Session | SecondFactorNeeded
        if ((await kindOf(username)) === 'e2ee') {
          const signed = await e2eeSignIn(username, password, device, forApp)
          answer = signed.answer as Session | SecondFactorNeeded
          sealed = signed.sealed
        } else
          answer = (await api('POST', '/auth/login', {
            username,
            password,
            device,
          })) as Session | SecondFactorNeeded
        if ('challenge' in answer) {
          second = answer
          password = ''
          recovery =
            !answer.methods.includes('totp') &&
            !answer.methods.includes('passkey') &&
            !answer.methods.includes('email')
        } else signedIn(answer)
      }
    })
  }
</script>

<main class="screen">
  {#if second}
    <form class="card" onsubmit={submit}>
      <h1>{t('admin.signIn.secondTitle')}</h1>
      {#if second.methods.includes('passkey') && !recovery}
        <p class="muted">{t('admin.signIn.secondPasskey')}</p>
        <button
          class="btn primary"
          type="button"
          disabled={busy || !passkeysSupported()}
          onclick={() => sendFactor('passkey')}
          ><Icon name="fingerprint" size={16} />{t('admin.signIn.usePasskey')}</button
        >
      {/if}
      {#if recovery}
        <label>
          {t('admin.signIn.recoveryCode')}
          <input
            class="field mono"
            autocomplete="off"
            spellcheck="false"
            required
            {@attach focus}
            bind:value={factorCode}
          />
        </label>
      {:else if byEmail}
        <p class="muted">{t('admin.signIn.emailCodeSent')}</p>
        <label>
          {t('admin.signIn.emailCode')}
          <input
            class="field mono"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength="7"
            required
            {@attach focus}
            bind:value={factorCode}
          />
        </label>
      {:else if second.methods.includes('totp')}
        <label>
          {t('admin.signIn.totpCode')}
          <input
            class="field mono"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength="7"
            required
            {@attach focus}
            bind:value={factorCode}
          />
        </label>
      {/if}
      {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
      {#if second.methods.includes('email') && !recovery && !byEmail}
        <button
          class={second.methods.length === 1 ||
          (second.methods.length === 2 && second.methods.includes('recoveryCode'))
            ? 'btn primary'
            : 'btn'}
          type="button"
          disabled={busy}
          onclick={sendEmailCode}>{t('admin.signIn.sendEmailCode')}</button
        >
      {/if}
      {#if recovery || byEmail || second.methods.includes('totp')}
        <button class="btn primary" type="submit" disabled={busy}>{t('admin.signIn.submit')}</button
        >
      {/if}
      <div class="links">
        {#if byEmail}
          <button class="link" type="button" disabled={busy} onclick={sendEmailCode}
            >{t('admin.signIn.resendEmailCode')}</button
          >
        {/if}
        {#if second.methods.includes('recoveryCode')}
          <button
            class="link"
            type="button"
            onclick={() => ((recovery = !recovery), (factorCode = ''), (problem = ''))}
          >
            {recovery ? t('admin.signIn.useFactor') : t('admin.signIn.useRecovery')}
          </button>
        {/if}
        <button class="link" type="button" onclick={startOver}>{t('admin.signIn.startOver')}</button
        >
      </div>
    </form>
  {:else if forgot}
    <form class="card" onsubmit={askReset}>
      <h1>{t('admin.forgot.title')}</h1>
      {#if resetAsked}
        <p role="status">{t('admin.forgot.sent')}</p>
      {:else if options?.emailReset}
        <p class="muted">{t('admin.forgot.help')}</p>
        <label>
          {t('admin.username')}
          <input
            class="field"
            autocomplete="username"
            spellcheck="false"
            required
            bind:value={username}
          />
        </label>
        {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
        <button class="btn primary" type="submit" disabled={busy}>{t('admin.forgot.send')}</button>
      {:else}
        <p class="muted">{t('admin.forgot.noEmail')}</p>
      {/if}
      <p class="muted">{t('admin.forgot.e2ee')}</p>
      <div class="links">
        <a class="link" href="recover">{t('admin.forgot.recover')}</a>
        <button
          class="link"
          type="button"
          onclick={() => ((forgot = false), (resetAsked = false), (problem = ''))}
          >{t('admin.forgot.back')}</button
        >
      </div>
    </form>
  {:else if mode === 'setup'}
    <div class="card">
      <h1>{t('admin.signIn.setupTitle')}</h1>
      <p class="muted">{t('admin.signIn.setupHelp')}</p>
      <a class="link" href="privacy">{t('admin.privacy.link')}</a>
    </div>
  {:else}
    <form class="card" onsubmit={submit}>
      <h1>
        {heading || t('admin.signIn.title', { app: brand.displayName })}
      </h1>
      {#if canPasskey}
        <button class="btn primary" type="button" disabled={busy} onclick={signInWithPasskey}
          ><Icon name="fingerprint" size={16} />{t('admin.signIn.withPasskey')}</button
        >
        <div class="or" aria-hidden="true">{t('admin.signIn.orPassword')}</div>
      {/if}
      <label>
        {t('admin.username')}
        <input
          class="field"
          autocomplete="username"
          spellcheck="false"
          required
          bind:value={username}
        />
      </label>
      <label>
        {t('admin.password')}
        <PasswordField
          class="field"
          autocomplete="current-password"
          required
          bind:value={password}
        />
      </label>
      {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
      <button class="btn" class:primary={!canPasskey} type="submit" disabled={busy}>
        {t('admin.signIn.submit')}
      </button>
      <div class="links">
        <button class="link" type="button" onclick={() => ((forgot = true), (problem = ''))}
          >{t('admin.forgot.link')}</button
        >
        <a class="link" href="privacy">{t('admin.privacy.link')}</a>
      </div>
    </form>
  {/if}
</main>

<style>
  .screen {
    min-height: 100%;
    display: grid;
    place-items: center;
    padding: var(--space-4);
  }
  .card {
    width: min(380px, 100%);
    display: grid;
    gap: var(--space-3);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  h1 {
    margin: 0;
    font-size: 20px;
  }
  p {
    margin: 0;
  }
  label {
    display: grid;
    gap: var(--space-1);
  }
  .mono {
    font-family: var(--font-ui);
    letter-spacing: 0.05em;
  }
  .problem {
    color: var(--color-danger);
  }
  .btn {
    justify-content: center;
  }
  .links {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-accent);
    text-decoration: underline;
  }
  .or {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .or::before,
  .or::after {
    content: '';
    flex: 1;
    border-top: 1px solid var(--color-border);
  }
</style>
