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
    auth,
    PROTOCOL_VERSION,
    route,
    webPath,
    type AuthOptions,
    type ServerInfo,
  } from './api.svelte'
  import { errorText } from './format'
  import Connect from './Connect.svelte'
  import Panel from './Panel.svelte'
  import Public from './Public.svelte'
  import Setup from './Setup.svelte'
  import SignIn from './SignIn.svelte'

  persistPrefs()
  applyTheme()
  $effect(() => void setLocale(prefs.language ?? systemLocale()))

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

  /** The web version's sign-in: the server shows this page at `/app/` (or
   * where its admin moved it) until
   * the browser holds the web version's cookie. */
  const webDoor = route() === webPath

  /** An app signing in through this page. */
  const connectPage = route() === '/connect' || route() === '/link'

  /** Signed in at the web version's door: the cookie, then the web version. */
  async function openWeb() {
    problem = ''
    try {
      await api('POST', '/web/session')
      location.reload()
    } catch (err) {
      problem = errorText(err)
    }
  }

  let info: ServerInfo | null = $state(null)
  let options: AuthOptions | null = $state(null)
  let problem = $state('')

  async function start() {
    problem = ''
    try {
      info = await api<ServerInfo>('GET', '/info')
      if (PROTOCOL_VERSION < info.minProtocol) problem = t('admin.errors.updateApp')
      else if (PROTOCOL_VERSION > info.maxProtocol) problem = t('admin.errors.updateServer')
      else {
        options = await api<AuthOptions>('GET', '/auth/options')
      }
    } catch (err) {
      problem = errorText(err)
    }
  }

  // already signed in on this tab: straight through the door
  $effect(() => {
    if (webDoor && auth.session && options && !problem) void openWeb()
  })

  void start()
</script>

{#if problem}
  <main class="center">
    <p role="alert">{problem}</p>
    <button class="btn" onclick={start}>{t('admin.retry')}</button>
  </main>
{:else if !info || !options}
  <main class="center" aria-busy="true"><p class="muted">{t('admin.loading')}</p></main>
{:else if connectPage}
  <Connect {options} byCode={route() === '/link'} />
{:else if webDoor && auth.session}
  <main class="center" aria-busy="true"><p class="muted">{t('admin.loading')}</p></main>
{:else if webDoor}
  <SignIn
    setup={false}
    {options}
    heading={t('admin.webDoor', { app: brand.displayName })}
    done={() => {
      // signed in: the effect above opens the web version
    }}
  />
{:else if setupPage}
  <Setup />
{:else if publicPage}
  <Public page={publicPage.page} token={publicPage.token} />
{:else if options.setupNeeded || !auth.session}
  <SignIn setup={options.setupNeeded} {options} done={start} />
{:else}
  <Panel {info} admin={auth.session.user.role === 'admin' && !auth.session.user.factorRequired} />
{/if}

<Overlays />

<style>
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
