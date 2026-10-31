<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Sync settings: connect this vault to a server account, see how sync
     stands, sync now or disconnect. Shared by the desktop and phone
     settings. -->
<script lang="ts">
  import { hour12 } from '../lib/clock.svelte'
  import PasswordField from '../lib/PasswordField.svelte'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import {
    isSyncError,
    sync,
    type SecondFactor,
    type ServerCheck,
    type SyncState,
  } from '../lib/sync.svelte'
  import SyncIcon from '../workspace/SyncIcon.svelte'
  import FilesSettings from './FilesSettings.svelte'

  /** Error codes with their own text, the others read "something went wrong". */
  const KNOWN = [
    'offline',
    'empty',
    'invalid',
    'insecure',
    'badCredentials',
    'lockedOut',
    'rateLimited',
    'accountDisabled',
    'secondFactorInvalid',
    'totpInvalid',
    'keys',
    'passkeyOnly',
    'certificate',
    'downgraded',
    'expired',
    'updateApp',
    'updateServer',
    'setupPending',
    'locked',
  ]

  const status = $derived(sync.status)
  const linked = $derived(status.state !== 'off')

  let address = $state('')
  let checked: ServerCheck | null = $state(null)
  /** A plain http server: the person said to use it anyway. */
  let plainOk = $state(false)
  const plain = $derived(
    ((checked as ServerCheck | null)?.address ?? status.server ?? '').startsWith('http://'),
  )
  let username = $state('')
  let password = $state('')
  let methods: SecondFactor[] | null = $state(null)
  let method: SecondFactor = $state('totp')
  let code = $state('')
  let busy = $state(false)
  let error = $state('')
  let confirming = $state(false)

  function explain(err: unknown) {
    if (!isSyncError(err)) return t('sync.error.other')
    if (err.code === 'lockedOut' && err.retryAfterSeconds)
      return t('sync.error.lockedOutFor', { minutes: Math.ceil(err.retryAfterSeconds / 60) })
    return KNOWN.includes(err.code) ? t(`sync.error.${err.code}`) : t('sync.error.other')
  }

  async function run(action: () => Promise<void>) {
    busy = true
    error = ''
    try {
      await action()
    } catch (err) {
      error = explain(err)
      console.error(err)
    } finally {
      busy = false
    }
  }

  function check(event: SubmitEvent) {
    event.preventDefault()
    void run(async () => {
      checked = await sync.checkServer(address)
      if (!checked.compatible) error = t('sync.error.incompatible')
    })
  }

  function signIn(event: SubmitEvent) {
    event.preventDefault()
    const server = checked?.address ?? status.server
    if (!server) return
    void run(async () => {
      const answer = await sync.signIn(server, username.trim(), password)
      password = ''
      if (answer.secondFactor) {
        methods = answer.secondFactor
        method = methods[0] ?? 'totp'
      } else {
        reset()
      }
    })
  }

  /** A code by email was asked for this sign-in. */
  let emailSent = $state(false)

  function sendEmailCode() {
    void run(async () => {
      await sync.emailCode()
      emailSent = true
    })
  }

  function giveCode(event: SubmitEvent) {
    event.preventDefault()
    void run(async () => {
      try {
        await sync.secondFactor(method, code)
        reset()
      } catch (err) {
        // a used or expired challenge needs the password again
        if (isSyncError(err) && err.code === 'expired') methods = null
        throw err
      } finally {
        code = ''
      }
    })
  }

  function reset() {
    checked = null
    plainOk = false
    methods = null
    emailSent = false
    password = ''
    code = ''
  }

  function disconnect() {
    confirming = false
    void run(() => sync.disconnect())
  }

  const STATE_TEXT: Record<SyncState, string> = {
    off: 'sync.state.off',
    signedOut: 'sync.state.signedOut',
    syncing: 'sync.state.syncing',
    offline: 'sync.state.offline',
    error: 'sync.state.error',
    pending: 'sync.state.pending',
    synced: 'sync.state.synced',
  }

  const when = (at: number) =>
    new Date(at).toLocaleString(currentLocale(), {
      dateStyle: 'medium',
      timeStyle: 'short',
      hour12: hour12(),
    })
</script>

{#if !sync.available}
  <p class="settings-help">{t('errors.unsupported')}</p>
{:else if linked && status.state !== 'signedOut'}
  <div class="settings-group wide">
    <div class="state">
      <SyncIcon state={status.state} />
      <div>
        <b>{t(STATE_TEXT[status.state])}</b>
        {#if status.state === 'pending' || status.state === 'offline'}
          <p class="settings-help">{t('sync.pendingCount', { count: status.pending })}</p>
        {/if}
        {#if status.state === 'error' && status.error}
          <p class="muted detail">{status.error}</p>
        {/if}
      </div>
    </div>
    <dl>
      <dt>{t('sync.server')}</dt>
      <dd>{status.server}</dd>
      <dt>{t('sync.account')}</dt>
      <dd>
        {status.username}
        <span class="kind">{status.e2ee ? t('sync.kindE2ee') : t('sync.kindStandard')}</span>
      </dd>
      <dt>{t('sync.lastSync')}</dt>
      <dd>{status.lastSyncAt ? when(status.lastSyncAt) : t('sync.never')}</dd>
      <dt>{t('sync.connection')}</dt>
      <dd>{status.live ? t('sync.live') : t('sync.everyMinute')}</dd>
    </dl>
  </div>
  <details class="settings-group more">
    <summary>{t('sync.more')}</summary>
    <label class="settings-check">
      <input
        type="checkbox"
        checked={prefs.coediting && status.coediting}
        disabled={!status.coediting}
        onchange={(e) => (prefs.coediting = e.currentTarget.checked)}
      />
      {t('sync.coediting')}
    </label>
    <p class="settings-help">
      {status.coediting ? t('sync.coeditingHelp') : t('sync.coeditingOff')}
    </p>
    <FilesSettings />
  </details>
  <div class="settings-group">
    {#if status.rejected > 0}
      <p role="alert">{t('sync.rejected', { count: status.rejected })}</p>
    {/if}
    <div class="settings-actions">
      <button class="btn" disabled={status.state === 'syncing'} onclick={() => void sync.now()}
        ><Icon name="syncing" size={16} />{t('sync.now')}</button
      >
      {#if confirming}
        <button class="btn danger" onclick={disconnect}>{t('sync.disconnectConfirm')}</button>
        <button class="btn" onclick={() => (confirming = false)}>{t('common.cancel')}</button>
      {:else}
        <button class="btn" onclick={() => (confirming = true)}>{t('sync.disconnect')}</button>
      {/if}
    </div>
    {#if confirming}<p class="settings-help">{t('sync.disconnectHelp')}</p>{/if}
  </div>
{:else if methods}
  <form class="settings-group" onsubmit={giveCode}>
    <p>{t('sync.secondFactorHelp')}</p>
    {#if methods.length > 1}
      <select class="field" bind:value={method} aria-label={t('sync.secondFactorMethod')}>
        {#each methods as m (m)}
          <option value={m}>{t(`sync.method.${m}`)}</option>
        {/each}
      </select>
    {/if}
    {#if method === 'email'}
      {#if emailSent}
        <p class="settings-help" role="status">{t('sync.emailCodeSent')}</p>
      {/if}
      <button class="btn" type="button" disabled={busy} onclick={sendEmailCode}
        >{emailSent ? t('sync.resendEmailCode') : t('sync.sendEmailCode')}</button
      >
    {/if}
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="field"
      autocomplete="one-time-code"
      inputmode={method === 'totp' || method === 'email' ? 'numeric' : 'text'}
      placeholder={t(`sync.method.${method}`)}
      autofocus
      bind:value={code}
    />
    <div class="settings-actions">
      <button class="btn primary" type="submit" disabled={busy || !code.trim()}
        >{t('sync.signIn')}</button
      >
      <button class="btn" type="button" onclick={reset}>{t('common.cancel')}</button>
    </div>
  </form>
{:else if checked?.compatible || status.state === 'signedOut'}
  <form class="settings-group" onsubmit={signIn}>
    {#if status.state === 'signedOut'}
      <p role="alert">{t('sync.signedOutHelp')}</p>
    {/if}
    <p class="settings-help">
      {t('sync.signInTo', { server: checked?.address ?? status.server ?? '' })}
    </p>
    {#if checked}
      <!-- what the check found: it answers, its version, who may sign up -->
      <dl>
        <dt>{t('sync.serverState')}</dt>
        <dd>{t('sync.serverUp')}</dd>
        <dt>{t('settings.version')}</dt>
        <dd>{checked.version}</dd>
        {#if checked.options}
          <dt>{t('sync.signUps')}</dt>
          <dd>
            {checked.options.registration === 'open'
              ? t('sync.signUpsOpen')
              : checked.options.registration === 'invite'
                ? t('sync.signUpsInvite')
                : t('sync.signUpsClosed')}
          </dd>
        {/if}
      </dl>
    {/if}
    <input
      class="field"
      autocomplete="username"
      placeholder={t('sync.username')}
      bind:value={username}
    />
    <PasswordField
      class="field"
      autocomplete="current-password"
      placeholder={t('sync.password')}
      bind:value={password}
    />
    {#if plain}
      <p role="alert">{t('sync.plainWarning')}</p>
      <label class="settings-check">
        <input type="checkbox" bind:checked={plainOk} />
        {t('sync.plainConfirm')}
      </label>
    {/if}
    <div class="settings-actions">
      <button
        class="btn primary"
        type="submit"
        disabled={busy || !username.trim() || !password || (plain && !plainOk)}
        >{busy ? t('sync.signingIn') : t('sync.signIn')}</button
      >
      {#if status.state === 'signedOut'}
        <button class="btn" type="button" onclick={() => (confirming = true)}
          >{t('sync.disconnect')}</button
        >
      {:else}
        <button class="btn" type="button" onclick={reset}>{t('common.back')}</button>
      {/if}
    </div>
    {#if confirming}
      <p class="settings-help">{t('sync.disconnectHelp')}</p>
      <div class="settings-actions">
        <button class="btn danger" type="button" onclick={disconnect}
          >{t('sync.disconnectConfirm')}</button
        >
        <button class="btn" type="button" onclick={() => (confirming = false)}
          >{t('common.cancel')}</button
        >
      </div>
    {/if}
    <p class="settings-help">{t('sync.passwordHelp')}</p>
  </form>
{:else}
  <form class="settings-group" onsubmit={check}>
    <p class="settings-help">{t('sync.intro')}</p>
    <input
      class="field"
      inputmode="url"
      autocomplete="url"
      placeholder={t('sync.address')}
      bind:value={address}
    />
    <div class="settings-actions">
      <button class="btn primary" type="submit" disabled={busy || !address.trim()}
        >{busy ? t('sync.checking') : t('sync.continue')}</button
      >
    </div>
  </form>
{/if}

{#if error}<p class="error" role="alert">{error}</p>{/if}

<style>
  .state {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
  }
  .state p {
    margin: var(--space-1) 0 0;
  }
  .detail {
    overflow-wrap: anywhere;
    font-size: 12px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-2) var(--space-4);
    margin: 0;
  }
  dt {
    color: var(--color-text-muted);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .kind {
    margin-left: var(--space-2);
    padding: 1px var(--space-2);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-control);
    font-size: 12px;
  }
  .error {
    margin: 0;
    color: var(--color-danger);
  }
</style>
