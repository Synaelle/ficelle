<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Root of the admin page: checks the server speaks our protocol, then
     shows the setup page, a public page, sign-in or the panel. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { setLocale, systemLocale, t } from '../lib/i18n.svelte'
  import Overlays from '../lib/Overlays.svelte'
  import { persistPrefs, prefs } from '../lib/prefs.svelte'
  import { applyTheme } from '../lib/theme.svelte'
  import {
    api,
    ApiError,
    auth,
    flash,
    keep,
    PROTOCOL_VERSION,
    route,
    signOut,
    thisDevice,
    type AuthOptions,
    type SecondFactorNeeded,
    type ServerInfo,
    type Session,
  } from './api.svelte'
  import { errorText } from './format'
  import Panel from './Panel.svelte'
  import Public from './Public.svelte'
  import Setup from './Setup.svelte'
  import SignIn from './SignIn.svelte'

  persistPrefs()
  applyTheme({ imported: false, downloads: false })
  $effect(() => setLocale(prefs.language ?? systemLocale()))

  /** A public page: a link from a mail, the recovery page, the privacy
   * page. They need no sign-in. */
  const publicPage = (() => {
    const path = route()
    const after = (prefix: string) => decodeURIComponent(path.slice(prefix.length))
    if (path.startsWith('/verify/')) return { page: 'verify' as const, token: after('/verify/') }
    if (path.startsWith('/reset/')) return { page: 'reset' as const, token: after('/reset/') }
    if (path === '/recover') return { page: 'recover' as const, token: '' }
    if (path === '/privacy') return { page: 'privacy' as const, token: '' }
    return null
  })()

  /** Opened from the setup link in the server log. */
  const setupPage = route() === '/setup'

  /** Opened from an invite link: `/invite/<token>`. */
  const invite = route().startsWith('/invite/')
    ? decodeURIComponent(route().slice('/invite/'.length))
    : null
  let registered = $state('')

  let info: ServerInfo | null = $state(null)
  let options: AuthOptions | null = $state(null)
  let problem = $state('')
  let ssoProblem = $state('')
  /** An admin back from single sign-on: this server's second factor next. */
  let ssoSecond: SecondFactorNeeded | null = $state(null)

  /** Back from the single sign-on provider: the fragment carries a code to
   * trade for a session, an error, or news of a linked identity. */
  async function arriveFromSso() {
    const hash = location.hash
    const value = (prefix: string) => {
      if (!hash.startsWith(prefix)) return null
      try {
        return decodeURIComponent(hash.slice(prefix.length))
      } catch {
        return null
      }
    }
    const code = value('#sso=')
    const error = value('#sso-error=')
    if (hash === '#sso-linked') {
      flash.account = t('admin.account.ssoLinked')
      history.replaceState(null, '', '#account')
    } else if (error) {
      // the address is anyone's to write: only a known code is shown, never
      // its text
      const known = new ApiError(0, error, t('admin.errors.ssoFailed'))
      if (auth.session) {
        flash.account = errorText(known)
        history.replaceState(null, '', '#account')
      } else {
        ssoProblem = errorText(known)
        history.replaceState(null, '', location.pathname)
      }
    } else if (code) {
      // the code works once, it leaves the address before anything else
      history.replaceState(null, '', location.pathname)
      try {
        const answer = await api<Session | SecondFactorNeeded>('POST', '/auth/sso/exchange', {
          code,
          device: thisDevice(),
        })
        if ('challenge' in answer) ssoSecond = answer
        else keep(answer)
      } catch (err) {
        ssoProblem = errorText(err)
      }
    }
  }

  async function start() {
    problem = ''
    try {
      info = await api<ServerInfo>('GET', '/info')
      if (PROTOCOL_VERSION < info.minProtocol) problem = t('admin.errors.updateApp')
      else if (PROTOCOL_VERSION > info.maxProtocol) problem = t('admin.errors.updateServer')
      else {
        await arriveFromSso()
        options = await api<AuthOptions>('GET', '/auth/options')
      }
    } catch (err) {
      problem = errorText(err)
    }
  }

  void start()
</script>

{#if problem}
  <main class="center">
    <p role="alert">{problem}</p>
    <button class="btn" onclick={start}>{t('admin.retry')}</button>
  </main>
{:else if !info || !options}
  <main class="center" aria-busy="true"><p class="muted">{t('admin.loading')}</p></main>
{:else if setupPage}
  <Setup />
{:else if publicPage}
  <Public page={publicPage.page} token={publicPage.token} />
{:else if registered}
  <main class="center">
    <h1>{t('admin.register.ready', { name: registered })}</h1>
    <p class="muted">{t('admin.register.readyHelp', { app: brand.displayName })}</p>
  </main>
{:else if invite}
  <SignIn
    setup={false}
    {invite}
    {options}
    done={async () => {
      // the account is for the app, this page only created it
      registered = auth.session?.user.username ?? ''
      await signOut()
    }}
  />
{:else if options.setupNeeded || !auth.session}
  <SignIn
    setup={options.setupNeeded}
    {options}
    initialProblem={ssoProblem}
    initialSecond={ssoSecond}
    done={start}
  />
{:else}
  <!-- members get their own account only, admins everything -->
  <Panel {info} admin={auth.session.user.role === 'admin' && !auth.session.user.factorRequired} />
{/if}

<Overlays />

<style>
  h1 {
    margin: 0;
    font-size: 20px;
  }
  .center {
    min-height: 100%;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: var(--space-3);
    padding: var(--space-4);
    text-align: center;
  }
</style>
