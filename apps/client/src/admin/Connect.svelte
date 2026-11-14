<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- An app signing in through this page. The app opened it with what the
     sign-in needs after the #: its PKCE challenge, a state, where to send
     the code, its device name and a one-time public key. The person signs
     in afresh, agrees, and the page hands the app a code through that
     address. An encrypted account's export key goes along, sealed to the
     app's key: the server passes it on and cannot open it. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { t } from '../lib/i18n.svelte'
  import { api, auth, base, signOut, type AuthOptions, type SecondFactorNeeded } from './api.svelte'
  import { errorText } from './format'
  import SignIn from './SignIn.svelte'

  let {
    options,
    initialProblem = '',
    initialSecond = null,
  }: {
    options: AuthOptions
    initialProblem?: string
    initialSecond?: SecondFactorNeeded | null
  } = $props()

  interface Request {
    challenge: string
    state: string
    redirect: string
    device: string
    platform: string
    key: string
  }

  /** Kept for the tab, a trip to single sign-on loses the address's #. */
  const KEY = 'connect-request'

  function fromAddress(): Request | null {
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

  /** A new request from the app, or the one this tab was already on. */
  const fresh = fromAddress()
  const request = fresh ?? saved()
  if (fresh) {
    try {
      sessionStorage.setItem(KEY, JSON.stringify(fresh))
    } catch {
      // without storage only single sign-on loses its way back
    }
    history.replaceState(null, '', location.pathname)
  }

  /** The export key sealed at sign-in, for an encrypted account. */
  let sealed: string | undefined = $state()
  let busy = $state(false)
  // svelte-ignore state_referenced_locally
  let problem = $state(initialProblem)
  /** Signing in again from the start, with this message. */
  let again = $state(0)
  let step: 'signIn' | 'ask' | 'sent' | 'cancelled' = $state('signIn')
  let sentTo = $state('')

  // a session this tab held before the app's new request is not one the
  // person just gave for it
  if (fresh && auth.session) void signOut()
  else if (auth.session) void signedIn()

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

  function back(params: Record<string, string>) {
    if (!request) return
    const query = new URLSearchParams({ ...params, state: request.state })
    sentTo = `${request.redirect}?${query}`
    try {
      sessionStorage.removeItem(KEY)
    } catch {
      // gone with the tab anyway
    }
    location.replace(sentTo)
  }

  async function connect() {
    if (!request) return
    busy = true
    problem = ''
    try {
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
    back({ error: 'access_denied' })
  }
</script>

{#if !request}
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
      returnTo={`${base}connect`}
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
      <p class="muted">{t('admin.connect.askOnly', { app: brand.displayName })}</p>
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
      {#if step === 'sent'}
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
</style>
