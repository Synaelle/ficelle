<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- An app signing in through this page, two ways. At /connect the app
     opened it with what the sign-in needs after the #: its PKCE challenge, a
     state, where to send the code, its device name and a one-time public
     key, and the page hands the app a code through that address. At /link
     the person types the code the app shows (or the app put it after the
     #), the page finds the app behind it, and the app, asking the server
     every few seconds, gets its session once the person agreed. Or the page
     shows a QR code, a phone scans it and asks for its code naming it, and
     the page, asking every few seconds, sees which app answered. Either way
     the person signs in afresh, and an encrypted account's export key goes
     along sealed to the app's key: the server passes it on and cannot open
     it. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { t } from '../lib/i18n.svelte'
  import {
    api,
    ApiError,
    auth,
    base,
    signOut,
    type AuthOptions,
    type SecondFactorNeeded,
  } from './api.svelte'
  import { errorText } from './format'
  import SignIn from './SignIn.svelte'

  let {
    options,
    byCode = false,
    initialProblem = '',
    initialSecond = null,
  }: {
    options: AuthOptions
    /** At /link: the app waits for a typed code. */
    byCode?: boolean
    initialProblem?: string
    initialSecond?: SecondFactorNeeded | null
  } = $props()

  interface Request {
    challenge: string
    device: string
    platform: string
    key: string
    /** With a redirect, at /connect. */
    state?: string
    redirect?: string
    /** With a code, at /link. */
    code?: string
    /** With a QR code a phone scanned, at /link. */
    offer?: string
  }

  interface LinkInfo {
    device: { name: string; platform: string }
    key: string
    challenge: string
  }

  /** Kept for the tab, a trip to single sign-on loses the address's #. */
  const KEY = 'connect-request'

  function fromAddress(): Request | null {
    if (byCode) return null
    const found = new URLSearchParams(location.hash.slice(1))
    const take = (name: string, longest: number) => {
      const value = found.get(name) ?? ''
      return value.length <= longest ? value : ''
    }
    const request = {
      challenge: take('challenge', 43),
      state: take('state', 128),
      redirect: take('redirect', 200),
      device: take('device', 100).trim(),
      platform: take('platform', 20),
      key: take('key', 100),
    }
    return Object.values(request).every(Boolean) ? request : null
  }

  function saved(): Request | null {
    try {
      const raw = sessionStorage.getItem(KEY)
      return raw ? (JSON.parse(raw) as Request) : null
    } catch {
      return null
    }
  }

  function keepRequest(found: Request) {
    try {
      sessionStorage.setItem(KEY, JSON.stringify(found))
    } catch {
      // without storage only single sign-on loses its way back
    }
  }

  /** The code the app put after the #, at /link. */
  // svelte-ignore state_referenced_locally
  const codeInAddress = byCode
    ? (new URLSearchParams(location.hash.slice(1)).get('code') ?? '').slice(0, 12)
    : ''

  /** A new request from the app, or the one this tab was already on. */
  const fresh = fromAddress()
  // svelte-ignore state_referenced_locally
  let request: Request | null = $state(
    codeInAddress ? null : (fresh ?? (saved()?.code || saved()?.offer || !byCode ? saved() : null)),
  )
  if (fresh) keepRequest(fresh)
  if (fresh || codeInAddress) history.replaceState(null, '', location.pathname)

  let typedCode = $state(codeInAddress)

  /** The export key sealed at sign-in, for an encrypted account. */
  let sealed: string | undefined = $state()
  let busy = $state(false)
  // svelte-ignore state_referenced_locally
  let problem = $state(initialProblem)
  /** Signing in again from the start, with this message. */
  let again = $state(0)
  /** What the page starts on, read once. */
  // svelte-ignore state_referenced_locally
  const typeFirst = byCode && !request
  let step: 'code' | 'qr' | 'signIn' | 'ask' | 'sent' | 'cancelled' = $state(
    typeFirst ? 'code' : 'signIn',
  )
  let sentTo = $state('')

  // a session this tab held before the app's new request is not one the
  // person just gave for it
  if ((fresh || typeFirst) && auth.session) void signOut()
  else if (auth.session && !typeFirst) void signedIn()

  /** The app behind a typed code, so its key is known before signing in. */
  async function findApp(event?: SubmitEvent) {
    event?.preventDefault()
    busy = true
    problem = ''
    try {
      const code = typedCode.trim()
      const info = await api<LinkInfo>('POST', '/auth/app/link/find', { code })
      request = {
        challenge: info.challenge,
        key: info.key,
        device: info.device.name,
        platform: info.device.platform,
        code,
      }
      keepRequest(request)
      step = 'signIn'
    } catch (err) {
      problem =
        err instanceof ApiError && err.code === 'linkInvalid'
          ? t('admin.connect.codeWrong', { app: brand.displayName })
          : errorText(err)
    } finally {
      busy = false
    }
  }
  if (codeInAddress) void findApp()

  // a phone's browser may bring this tab back for the app's next code and
  // only change what follows the #: that is a new sign-in
  $effect(() => {
    if (!byCode) return
    const again = () => {
      const code = new URLSearchParams(location.hash.slice(1)).get('code')
      if (!code) return
      history.replaceState(null, '', location.pathname)
      void signOut()
      forget()
      request = null
      sealed = undefined
      typedCode = code.slice(0, 12)
      step = 'code'
      void findApp()
    }
    window.addEventListener('hashchange', again)
    return () => window.removeEventListener('hashchange', again)
  })

  /** The QR code shown, its token after `#offer=` in what it holds. */
  let shown: { offer: string; qr: string; expiresAt: number } | null = $state(null)
  let qrExpired = $state(false)

  async function showQr() {
    busy = true
    problem = ''
    qrExpired = false
    try {
      const made = await api<{ link: string; qr: string; expiresAt: number }>(
        'POST',
        '/auth/app/link/offer',
      )
      shown = { offer: made.link.split('#offer=')[1] ?? '', qr: made.qr, expiresAt: made.expiresAt }
      step = 'qr'
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  // while the QR code shows, which app scanned it
  $effect(() => {
    if (step !== 'qr' || !shown || qrExpired) return
    const { offer, expiresAt } = shown
    let stopped = false
    const ask = async () => {
      if (stopped) return
      if (Date.now() > expiresAt) {
        qrExpired = true
        return
      }
      try {
        const { app } = await api<{ app?: LinkInfo }>('POST', '/auth/app/link/offer/status', {
          offer,
        })
        if (stopped) return
        if (app) {
          request = {
            challenge: app.challenge,
            key: app.key,
            device: app.device.name,
            platform: app.device.platform,
            offer,
          }
          keepRequest(request)
          step = 'signIn'
          return
        }
      } catch (err) {
        if (stopped) return
        if (err instanceof ApiError && err.code === 'linkInvalid') {
          qrExpired = true
          return
        }
        // a dropped connection: the next question tries again
      }
      timer = setTimeout(ask, 3000)
    }
    let timer = setTimeout(ask, 3000)
    return () => {
      stopped = true
      clearTimeout(timer)
    }
  })

  async function signedIn(key?: string) {
    sealed = key
    // a passkey or single sign-on has no password: an encrypted account's
    // notes cannot be opened without it
    if (auth.session?.user.kind === 'e2ee' && !sealed) {
      await signOut()
      problem = t('admin.connect.needPassword', { app: brand.displayName })
      again += 1
      return
    }
    problem = ''
    step = 'ask'
  }

  function forget() {
    try {
      sessionStorage.removeItem(KEY)
    } catch {
      // gone with the tab anyway
    }
  }

  function back(params: Record<string, string>) {
    if (!request?.redirect || !request.state) return
    const query = new URLSearchParams({ ...params, state: request.state })
    sentTo = `${request.redirect}?${query}`
    forget()
    location.replace(sentTo)
  }

  async function connect() {
    if (!request) return
    busy = true
    problem = ''
    try {
      if (request.code || request.offer) {
        await (request.offer
          ? api('POST', '/auth/app/link/offer/approve', { offer: request.offer, sealed })
          : api('POST', '/auth/app/link/approve', { code: request.code, sealed }))
        // the app asks the server for its session, this page's is done
        await signOut()
        forget()
        step = 'sent'
        return
      }
      const { code } = await api<{ code: string }>('POST', '/auth/app/code', {
        redirect: request.redirect,
        challenge: request.challenge,
        device: { name: request.device, platform: request.platform },
        sealed,
      })
      // the app gets its own session for the code, this page's is done
      await signOut()
      step = 'sent'
      back({ code })
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  async function cancel() {
    await signOut()
    step = 'cancelled'
    forget()
    back({ error: 'access_denied' })
  }
</script>

{#if step === 'code'}
  <main class="screen">
    <form class="card" onsubmit={findApp}>
      <h1>{t('admin.connect.codeTitle', { app: brand.displayName })}</h1>
      <p class="muted">{t('admin.connect.codeHelp', { app: brand.displayName })}</p>
      <label>
        {t('admin.connect.code')}
        <input
          class="field mono"
          autocomplete="off"
          autocapitalize="characters"
          spellcheck="false"
          maxlength="9"
          placeholder="ABCD-EFGH"
          required
          bind:value={typedCode}
        />
      </label>
      {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
      <button class="btn primary" type="submit" disabled={busy}
        >{t('admin.connect.continue')}</button
      >
      <button class="btn" type="button" disabled={busy} onclick={showQr}
        >{t('admin.connect.qrButton')}</button
      >
    </form>
  </main>
{:else if step === 'qr' && shown}
  <main class="screen">
    <div class="card">
      <h1>{t('admin.connect.qrTitle', { app: brand.displayName })}</h1>
      {#if qrExpired}
        <p role="alert">{t('admin.connect.qrExpired')}</p>
        <button class="btn primary" type="button" disabled={busy} onclick={showQr}
          >{t('admin.connect.qrAgain')}</button
        >
      {:else}
        <img
          class="qr"
          src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(shown.qr)}`}
          alt={t('admin.connect.qrAlt', { app: brand.displayName })}
          width="240"
          height="240"
        />
        <p class="muted">{t('admin.connect.qrHelp', { app: brand.displayName })}</p>
      {/if}
      {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
    </div>
  </main>
{:else if !request}
  <main class="center">
    <p role="alert">{t('admin.connect.broken', { app: brand.displayName })}</p>
  </main>
{:else if step === 'signIn'}
  {#key again}
    <SignIn
      setup={false}
      {options}
      heading={t('admin.connect.title', { app: brand.displayName })}
      forApp={{ key: request.key, challenge: request.challenge }}
      returnTo={`${base}${byCode ? 'link' : 'connect'}`}
      initialProblem={problem}
      initialSecond={again ? null : initialSecond}
      done={signedIn}
    />
  {/key}
{:else if step === 'ask'}
  <main class="screen">
    <div class="card">
      <h1>{t('admin.connect.askTitle', { device: request.device })}</h1>
      <p>
        {t('admin.connect.askHelp', {
          device: request.device,
          server: location.host,
          name: auth.session?.user.username ?? '',
        })}
      </p>
      <p class="muted">
        {request.offer
          ? t('admin.connect.askOnlyQr')
          : t('admin.connect.askOnly', { app: brand.displayName })}
      </p>
      {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
      <div class="row">
        <button class="btn" type="button" disabled={busy} onclick={cancel}
          >{t('admin.connect.cancel')}</button
        >
        <button class="btn primary" type="button" disabled={busy} onclick={connect}
          >{t('admin.connect.connect')}</button
        >
      </div>
    </div>
  </main>
{:else}
  <main class="screen">
    <div class="card" role="status">
      <h1>
        {step === 'sent'
          ? t('admin.connect.doneTitle', { app: brand.displayName })
          : t('admin.connect.cancelled')}
      </h1>
      <p class="muted">{t('admin.connect.doneHelp')}</p>
      {#if step === 'sent' && sentTo}
        <!-- a browser that held back the automatic switch still follows a tap -->
        <a class="btn" href={sentTo}>{t('admin.connect.openApp', { app: brand.displayName })}</a>
      {/if}
    </div>
  </main>
{/if}

<style>
  .screen,
  .center {
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
    overflow-wrap: anywhere;
  }
  p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .problem {
    color: var(--color-danger);
  }
  .row {
    display: flex;
    justify-content: end;
    gap: var(--space-2);
  }
  .btn {
    justify-content: center;
  }
  label {
    display: grid;
    gap: var(--space-1);
  }
  .qr {
    justify-self: center;
    width: min(240px, 100%);
    height: auto;
    border-radius: var(--radius-control);
    background: #fff;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 18px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
</style>
