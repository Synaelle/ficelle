<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Sign-in to the admin page: a passkey first when the server and the
     browser have them, else a password and maybe a second factor, or single
     sign-on. An end-to-end encrypted account signs in over OPAQUE, its
     password never leaves the page. On a server not set up yet it points to
     the setup link in the server log. Opened from an invite link, it creates
     that person's account, standard or end-to-end encrypted. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import { brand } from '@app/brand'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import {
    api,
    ApiError,
    base,
    keep,
    thisDevice,
    type AccountKind,
    type AuthOptions,
    type PasskeyCeremony,
    type SecondFactorNeeded,
    type Session,
  } from './api.svelte'
  import { kindOf, signIn as e2eeSignIn, signUp as e2eeSignUp } from './e2ee'
  import { errorText } from './format'
  import { passkeysSupported, usePasskey } from './webauthn'

  let {
    setup,
    invite = null,
    options = null,
    initialProblem = '',
    initialSecond = null,
    done,
  }: {
    setup: boolean
    invite?: string | null
    options?: AuthOptions | null
    initialProblem?: string
    /** Back from single sign-on as an admin: this server's second factor. */
    initialSecond?: SecondFactorNeeded | null
    done: () => void
  } = $props()

  const mode = $derived(invite ? 'register' : setup ? 'setup' : 'signIn')
  const newAccount = $derived(mode !== 'signIn')

  let username = $state('')
  let password = $state('')
  let again = $state('')
  let busy = $state(false)
  // svelte-ignore state_referenced_locally
  let problem = $state(initialProblem)

  /** Set once the password was right and a second factor is due. */
  // svelte-ignore state_referenced_locally
  let second: SecondFactorNeeded | null = $state(initialSecond)
  let factorCode = $state('')
  // svelte-ignore state_referenced_locally
  let recovery = $state(
    !!initialSecond &&
      !initialSecond.methods.includes('totp') &&
      !initialSecond.methods.includes('passkey') &&
      !initialSecond.methods.includes('email'),
  )
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

  const canPasskey = $derived(!!options?.passkeys && passkeysSupported())

  /** The kind of the new account. Only asked when the server lets the
   * person choose. */
  let chosen: AccountKind = $state('standard')
  const newKind: AccountKind = $derived(
    mode === 'register' && options?.accountKinds === 'e2ee' ? 'e2ee' : chosen,
  )
  const choosing = $derived(mode === 'register' && options?.accountKinds === 'choice')

  /** An e2ee account just made: its recovery code, shown once, then the
   * session. */
  let made = $state<{ code: string; session: Session } | null>(null)
  let typedBack = $state('')
  let copied = $state(false)
  const lastGroup = $derived(made?.code.split('-').at(-1) ?? '')

  async function copyCode() {
    try {
      await navigator.clipboard.writeText(made?.code ?? '')
      copied = true
      setTimeout(() => (copied = false), 2000)
    } catch {
      problem = t('admin.invites.copyFailed')
    }
  }

  function saveCode() {
    if (!made) return
    const text = `${t('admin.register.codeFile', { app: brand.displayName, host: location.host, name: made.session.user.username })}\n\n${made.code}\n`
    const link = document.createElement('a')
    link.href = URL.createObjectURL(new Blob([text], { type: 'text/plain' }))
    link.download = `${brand.slug}-recovery-code.txt`
    link.click()
    URL.revokeObjectURL(link.href)
  }

  function keptIt(event: SubmitEvent) {
    event.preventDefault()
    if (!made) return
    if (typedBack.replace(/[\s-]/g, '').toUpperCase() !== lastGroup) {
      problem = t('admin.register.typeBackWrong')
      return
    }
    const { session } = made
    made = null
    signedIn(session)
  }

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

  function signedIn(session: Session) {
    keep(session)
    done()
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

  async function signInWithSso() {
    await attempt(async () => {
      const { url } = await api<{ url: string }>('POST', '/auth/sso/start', {
        returnTo: `${base}admin`,
      })
      location.assign(url)
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
    factorCode = ''
    recovery = false
    byEmail = false
    problem = ''
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    problem = ''
    if (newAccount && password !== again) {
      problem = t('admin.signIn.mismatch')
      return
    }
    if (second) {
      await sendFactor(recovery ? 'recoveryCode' : byEmail ? 'email' : 'totp')
      return
    }
    await attempt(async () => {
      const device = thisDevice()
      if (mode === 'register' && newKind === 'e2ee') {
        const account = await e2eeSignUp(username, password, { invite, device })
        made = { code: account.recoveryCode, session: account.session }
        password = ''
        again = ''
      } else if (mode === 'register')
        signedIn(await api('POST', '/auth/register', { username, password, invite, device }))
      else {
        const answer = (
          (await kindOf(username)) === 'e2ee'
            ? await e2eeSignIn(username, password, device)
            : await api('POST', '/auth/login', { username, password, device })
        ) as Session | SecondFactorNeeded
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
  {#if made}
    <form class="card" onsubmit={keptIt}>
      <h1>{t('admin.register.codeTitle')}</h1>
      <p>{t('admin.register.codeHelp')}</p>
      <code class="code">{made.code}</code>
      <div class="row">
        <button class="btn" type="button" onclick={copyCode}
          ><Icon name={copied ? 'check' : 'copy'} size={16} />{copied
            ? t('admin.invites.copied')
            : t('admin.invites.copy')}</button
        >
        <button class="btn" type="button" onclick={saveCode}
          ><Icon name="export" size={16} />{t('admin.account.saveTxt')}</button
        >
      </div>
      <label>
        {t('admin.register.typeBack')}
        <input
          class="field mono"
          autocomplete="off"
          spellcheck="false"
          maxlength="4"
          required
          bind:value={typedBack}
        />
      </label>
      {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
      <button class="btn primary" type="submit">{t('admin.register.keptIt')}</button>
    </form>
  {:else if second}
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
        {mode === 'register'
          ? t('admin.register.title', { app: brand.displayName })
          : t('admin.signIn.title', { app: brand.displayName })}
      </h1>
      {#if mode === 'signIn' && canPasskey}
        <button class="btn primary" type="button" disabled={busy} onclick={signInWithPasskey}
          ><Icon name="fingerprint" size={16} />{t('admin.signIn.withPasskey')}</button
        >
        <p class="muted small">{t('admin.signIn.passkeyHelp')}</p>
        <div class="or" aria-hidden="true">{t('admin.signIn.orPassword')}</div>
      {/if}
      {#if mode === 'register'}
        <p class="muted">{t('admin.register.help')}</p>
      {/if}
      {#if choosing}
        <fieldset class="kinds">
          <legend>{t('admin.register.kind')}</legend>
          {#each ['standard', 'e2ee'] as const as kind (kind)}
            <label class="kind" class:checked={chosen === kind}>
              <input type="radio" name="kind" value={kind} bind:group={chosen} />
              <span>
                <b>{t(`admin.kinds.${kind}`)}</b>
                <span class="muted">{t(`admin.kinds.${kind}Help`)}</span>
              </span>
            </label>
          {/each}
        </fieldset>
      {/if}
      {#if mode === 'register' && newKind === 'e2ee'}
        <p class="warn">{t('admin.register.e2eeWarning')}</p>
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
          autocomplete={newAccount ? 'new-password' : 'current-password'}
          required
          bind:value={password}
        />
      </label>
      {#if newAccount}
        <label>
          {t('admin.signIn.again')}
          <PasswordField
            eye={false}
            class="field"
            autocomplete="new-password"
            required
            bind:value={again}
          />
        </label>
      {/if}
      {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
      <button
        class="btn"
        class:primary={mode !== 'signIn' || !canPasskey}
        type="submit"
        disabled={busy}
      >
        {mode === 'register' ? t('admin.register.submit') : t('admin.signIn.submit')}
      </button>
      {#if mode === 'signIn' && options?.sso}
        <div class="or" aria-hidden="true">{t('admin.signIn.or')}</div>
        {#if options?.sso}
          <button class="btn" type="button" disabled={busy} onclick={signInWithSso}
            ><Icon name="key" size={16} />{options.sso}</button
          >
        {/if}
      {/if}
      {#if mode === 'signIn'}
        <div class="links">
          <button class="link" type="button" onclick={() => ((forgot = true), (problem = ''))}
            >{t('admin.forgot.link')}</button
          >
          <a class="link" href="privacy">{t('admin.privacy.link')}</a>
        </div>
      {:else}
        <a class="link" href="privacy">{t('admin.privacy.link')}</a>
      {/if}
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
    font-family: var(--font-mono);
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
  .kinds {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
  }
  .kinds legend {
    margin-bottom: var(--space-1);
  }
  .kind {
    display: flex;
    align-items: start;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
  }
  .kind.checked {
    border-color: var(--color-accent);
  }
  .kind span {
    display: grid;
    font-size: 13px;
  }
  .warn {
    padding: var(--space-2) var(--space-3);
    border-left: 3px solid var(--color-accent);
    font-size: 13px;
  }
  .code {
    padding: var(--space-3);
    border: 1px solid var(--color-accent);
    border-radius: var(--radius-control);
    font-family: var(--font-mono);
    font-size: 17px;
    letter-spacing: 0.04em;
    text-align: center;
    overflow-wrap: anywhere;
    user-select: all;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
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
  .small {
    margin: 0;
    font-size: 13px;
  }
</style>
