<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Root of the admin page: checks the server speaks our protocol, then
     shows the setup page, a public page, sign-in or the panel. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import Overlays from '../lib/Overlays.svelte'
  import { persistPrefs } from '../lib/prefs.svelte'
  import { applyTheme } from '../lib/theme.svelte'
  import {
    api,
    auth,
    PROTOCOL_VERSION,
    route,
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

  /** A public page: the recovery page, the privacy page. They need no
   * sign-in. */
  const publicPage = (() => {
    const path = route()
    if (path === '/recover') return 'recover' as const
    if (path === '/privacy') return 'privacy' as const
    return null
  })()

  /** Opened from the setup link in the server log. */
  const setupPage = route() === '/setup'

  /** An app signing in through this page. */
  const connectPage = route() === '/connect' || route() === '/link'

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
{:else if setupPage}
  <Setup />
{:else if publicPage}
  <Public page={publicPage} />
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
