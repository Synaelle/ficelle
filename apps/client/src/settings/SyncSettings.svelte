<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Sync settings, shared by the desktop and phone settings. First sync
     between the person's own devices on the same network: this device's
     name, the paired devices, pairing, and a page per device. Then the sync
     server, deprecated, folded away and open by itself on a device linked to
     one: connect to an account, see how sync stands, sync now or
     disconnect. A device uses one or the other, never both. -->
<script lang="ts">
  import { hour12 } from '../lib/clock.svelte'
  import PasswordField from '../lib/PasswordField.svelte'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import { protocolGap, protocolRange, versionGap } from '../lib/serverVersion'
  import Icon from '../lib/icons/Icon.svelte'
  import {
    isSyncError,
    sync,
    type SecondFactor,
    type ServerCheck,
    type SyncState,
  } from '../lib/sync.svelte'
  import SyncIcon from '../workspace/SyncIcon.svelte'
  import { localSync, type LocalDevice } from '../lib/localSync.svelte'
  import FilesSettings from './FilesSettings.svelte'
  import PairDialog from './PairDialog.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Switch from './ui/Switch.svelte'

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
    'devicesPaired',
  ]

  const status = $derived(sync.status)
  const linked = $derived(status.state !== 'off')
  /** The server part, folded at first unless this device uses one, then as
   * the person chooses. */
  let chosen: boolean | null = $state(null)
  const serverOpen = $derived(chosen ?? linked)

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

  /** The linked server as last heard, null until it answers. */
  let linkedServer: ServerCheck | null = $state(null)
  $effect(() => {
    const server = linked ? status.server : null
    linkedServer = null
    if (!server) return
    let left = false
    sync
      .checkServer(server)
      .then((answer) => {
        if (!left) linkedServer = answer
      })
      .catch(() => {})
    return () => {
      left = true
    }
  })

  const protocolProblem = $derived.by(() => {
    const server = linkedServer as ServerCheck | null
    return server && protocolGap(__APP_PROTOCOL__, server.minProtocol, server.maxProtocol)
  })

  /** The advice about versions, none when a protocol problem says more. */
  function versionNote(server: ServerCheck | null): string | undefined {
    if (!server) return undefined
    if (protocolGap(__APP_PROTOCOL__, server.minProtocol, server.maxProtocol)) return undefined
    const gap = versionGap(__APP_VERSION__, server.version)
    return gap ? t(`sync.version_${gap}`) : undefined
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

  const local = $derived(localSync.status)
  const devicesPaired = $derived(local.devices.length > 0)
  /** The pairing dialog open, and which way. */
  let pairMode: 'show' | 'type' | null = $state(null)
  let renaming: string | null = $state(null)
  /** The device whose page is open, by key. */
  let viewingKey: string | null = $state(null)
  const viewing = $derived(local.devices.find((d) => d.key === viewingKey) ?? null)
  let forgetting = $state(false)
  let localError = $state('')

  function rename(event: SubmitEvent) {
    event.preventDefault()
    const name = renaming?.trim()
    if (!name) return
    localSync
      .rename(name)
      .then(() => (renaming = null))
      .catch(() => (localError = t('local.error.other')))
  }

  function forget(device: LocalDevice) {
    forgetting = false
    localSync
      .forget(device.key)
      .then(() => (viewingKey = null))
      .catch(() => (localError = t('local.error.other')))
  }

  /** What a device row says about it. */
  function deviceLine(device: LocalDevice): string {
    if (device.state === 'away' || device.state === 'waiting') {
      return device.lastSyncAt
        ? t('local.lastSynced', { when: when(device.lastSyncAt) })
        : t('local.neverSynced')
    }
    return t(`local.state.${device.state}`)
  }

  const platformName = (p: string) =>
    ['windows', 'linux', 'macos', 'android'].includes(p) ? t(`local.platform.${p}`) : p

  const when = (at: number) =>
    new Date(at).toLocaleString(currentLocale(), {
      dateStyle: 'medium',
      timeStyle: 'short',
      hour12: hour12(),
    })
</script>

{#if !localSync.available}
  <p class="settings-help">{t('errors.unsupported')}</p>
{:else if viewing}
  <div class="device-head">
    <button class="icon-btn" aria-label={t('common.back')} onclick={() => (viewingKey = null)}
      ><Icon name="back" size={16} /></button
    >
    <h3>{viewing.name}</h3>
  </div>
  {#if viewing.state === 'needsUpdate' || viewing.state === 'updateThis'}
    <div class="notice" role="alert">
      <Icon name="warning" size={18} />
      <div>
        <b
          >{viewing.state === 'needsUpdate'
            ? t('local.device.needsUpdate', { name: viewing.name })
            : t('local.device.updateThis')}</b
        >
        <p>
          {viewing.state === 'needsUpdate'
            ? t('local.device.needsUpdateHelp', { name: viewing.name })
            : t('local.device.updateThisHelp', { name: viewing.name })}
        </p>
      </div>
    </div>
  {/if}
  <Group title={t('local.device.versions')}>
    <Row label={t('local.device.thisDevice')}><span class="value mono">{__APP_VERSION__}</span></Row
    >
    <Row label={viewing.name}
      ><span class="value mono" class:warn={viewing.state === 'needsUpdate'}
        >{viewing.detail && viewing.state !== 'failed'
          ? viewing.detail
          : viewing.version || '?'}</span
      ></Row
    >
  </Group>
  <Group note={viewing.state === 'failed' && viewing.detail ? viewing.detail : undefined}>
    <Row label={t('local.device.platform')}
      ><span class="value">{platformName(viewing.platform)}</span></Row
    >
    <Row label={t('local.device.lastSync')}>
      <span class="value">{viewing.lastSyncAt ? when(viewing.lastSyncAt) : t('sync.never')}</span>
    </Row>
    <Row label={t('local.state.label')}><span class="value">{deviceLine(viewing)}</span></Row>
    <Row label={t('sync.now')}>
      <button
        class="btn"
        disabled={viewing.state === 'needsUpdate' || viewing.state === 'updateThis'}
        onclick={() => void localSync.now()}
        ><Icon name="syncing" size={16} />{t('sync.now')}</button
      >
    </Row>
  </Group>
  <Group note={t('local.device.forgetHelp')}>
    <Row label={t('local.device.forget')}>
      {#if forgetting}
        <button class="btn" onclick={() => (forgetting = false)}>{t('common.cancel')}</button>
        <button class="btn danger" onclick={() => viewing && forget(viewing)}
          >{t('local.device.forgetConfirm', { name: viewing.name })}</button
        >
      {:else}
        <button class="btn danger" onclick={() => (forgetting = true)}
          >{t('local.device.forgetButton')}</button
        >
      {/if}
    </Row>
  </Group>
{:else}
  <p class="settings-help">{t('local.intro')}</p>

  {#if local.open}
    <Group title={t('local.thisDevice')} note={t('local.nameHelp')}>
      {#if renaming !== null}
        <form class="field-row rename" onsubmit={rename}>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="field"
            aria-label={t('local.name')}
            maxlength="60"
            autofocus
            bind:value={renaming}
          />
          <button class="btn" type="button" onclick={() => (renaming = null)}
            >{t('common.cancel')}</button
          >
          <button class="btn primary" type="submit" disabled={!renaming.trim()}
            >{t('local.save')}</button
          >
        </form>
      {:else}
        <Row label={t('local.name')}>
          <span class="value">{local.name}</span>
          <button
            class="icon-btn"
            aria-label={t('local.rename')}
            disabled={local.serverLinked}
            onclick={() => (renaming = local.name)}><Icon name="pencil" size={16} /></button
          >
        </Row>
      {/if}
    </Group>

    <Group
      title={t('local.devices')}
      note={local.serverLinked
        ? t('local.oneWay')
        : devicesPaired
          ? t('local.phoneHelp')
          : undefined}
    >
      {#if local.serverLinked}
        <div class="settings-row empty">{t('local.serverInUse')}</div>
      {:else if !devicesPaired}
        <div class="settings-row empty">
          <span>{t('local.none')}</span>
          <span class="muted">{t('local.noneHelp')}</span>
        </div>
      {:else}
        {#each local.devices as device (device.key)}
          <Row
            label={device.name}
            help={platformName(device.platform)}
            opens
            onclick={() => (viewingKey = device.key)}
          >
            <span
              class="device-state"
              class:ok={device.state === 'synced'}
              class:warn={device.state === 'needsUpdate' || device.state === 'updateThis'}
              class:bad={device.state === 'failed'}>{deviceLine(device)}</span
            >
          </Row>
        {/each}
      {/if}
    </Group>
    <div class="settings-buttons start">
      <button class="btn primary" disabled={local.serverLinked} onclick={() => (pairMode = 'show')}
        ><Icon name="plus" size={16} />{t('local.pair')}</button
      >
      <button class="btn" disabled={local.serverLinked} onclick={() => (pairMode = 'type')}
        >{t('local.haveCode')}</button
      >
    </div>
    {#if local.discoveryError && local.running}
      <p class="settings-help">{t('local.discoveryProblem', { error: local.discoveryError })}</p>
    {/if}
    {#if localError}<p class="error" role="alert">{localError}</p>{/if}
    {#if !linked}<FilesSettings local />{/if}
  {/if}
{/if}

{#if pairMode}
  <PairDialog mode={pairMode} close={() => (pairMode = null)} />
{/if}

{#if !viewing}
  <Group>
    <Row label={t('sync.serverSection')} onclick={() => (chosen = !serverOpen)}>
      <span class="chevron" class:open={serverOpen}><Icon name="down" size={16} /></span>
    </Row>
  </Group>
{/if}

{#if serverOpen && !viewing}
  <div class="fold-body">
    {#if !sync.available}
      <p class="settings-help">{t('errors.unsupported')}</p>
    {:else if devicesPaired && !linked}
      <p class="settings-help">{t('sync.serverDeprecated')}</p>
      <p class="settings-help">{t('local.devicesInUse')}</p>
    {:else if linked && status.state !== 'signedOut'}
      <p class="settings-help">{t('sync.serverDeprecated')}</p>
      <Group>
        <div class="settings-row state">
          <SyncIcon state={status.state} />
          <div>
            <b>{t(STATE_TEXT[status.state])}</b>
            {#if status.state === 'pending' || status.state === 'offline'}
              <p class="settings-help">{t('sync.pendingCount', { count: status.pending })}</p>
            {/if}
            {#if status.state === 'error' && status.error}
              <p class="settings-help detail">{status.error}</p>
            {/if}
            {#if status.rejected > 0}
              <p role="alert">{t('sync.rejected', { count: status.rejected })}</p>
            {/if}
          </div>
          <button class="btn" disabled={status.state === 'syncing'} onclick={() => void sync.now()}
            ><Icon name="syncing" size={16} />{t('sync.now')}</button
          >
        </div>
        <Row label={t('sync.server')}><span class="value">{status.server}</span></Row>
        <Row label={t('sync.account')}>
          <span class="value">{status.username}</span>
          <span class="kind">{status.e2ee ? t('sync.kindE2ee') : t('sync.kindStandard')}</span>
        </Row>
        <Row label={t('sync.lastSync')}>
          <span class="value">{status.lastSyncAt ? when(status.lastSyncAt) : t('sync.never')}</span>
        </Row>
        <Row label={t('sync.connection')}>
          <span class="value">{status.live ? t('sync.live') : t('sync.everyMinute')}</span>
        </Row>
      </Group>
      {#if linkedServer}
        {#if protocolProblem}<p class="error" role="alert">
            {t(`sync.error.${protocolProblem}`)}
          </p>{/if}
        <Group title={t('sync.versions')} note={versionNote(linkedServer)}>
          <Row label={t('sync.thisApp')}>
            <span class="value"
              >{t('sync.versionLine', {
                version: __APP_VERSION__,
                protocol: __APP_PROTOCOL__,
              })}</span
            >
          </Row>
          <Row label={t('sync.server')}>
            <span class="value"
              >{t('sync.versionLine', {
                version: linkedServer.version,
                protocol: protocolRange(linkedServer.minProtocol, linkedServer.maxProtocol) ?? '?',
              })}</span
            >
          </Row>
        </Group>
      {/if}
      <Group
        title={t('sync.more')}
        note={status.coediting ? t('sync.coeditingHelp') : t('sync.coeditingOff')}
      >
        <Row label={t('sync.coediting')}>
          <Switch
            label={t('sync.coediting')}
            checked={prefs.coediting && status.coediting}
            disabled={!status.coediting}
            onchange={(on) => (prefs.coediting = on)}
          />
        </Row>
      </Group>
      <FilesSettings />
      <Group note={confirming ? t('sync.disconnectHelp') : undefined}>
        <Row label={t('sync.disconnect')}>
          {#if confirming}
            <button class="btn" onclick={() => (confirming = false)}>{t('common.cancel')}</button>
            <button class="btn danger" onclick={disconnect}>{t('sync.disconnectConfirm')}</button>
          {:else}
            <button class="btn" onclick={() => (confirming = true)}>{t('sync.disconnect')}</button>
          {/if}
        </Row>
      </Group>
    {:else if methods}
      <form class="sync-form" onsubmit={giveCode}>
        <p class="settings-help">{t('sync.secondFactorHelp')}</p>
        <Group note={method === 'email' && emailSent ? t('sync.emailCodeSent') : undefined}>
          {#if methods.length > 1}
            <Row label={t('sync.secondFactorMethod')}>
              <select
                class="field compact"
                bind:value={method}
                aria-label={t('sync.secondFactorMethod')}
              >
                {#each methods as m (m)}
                  <option value={m}>{t(`sync.method.${m}`)}</option>
                {/each}
              </select>
            </Row>
          {/if}
          <div class="field-row">
            <!-- svelte-ignore a11y_autofocus -->
            <input
              class="field"
              autocomplete="one-time-code"
              inputmode={method === 'totp' || method === 'email' ? 'numeric' : 'text'}
              placeholder={t(`sync.method.${method}`)}
              aria-label={t(`sync.method.${method}`)}
              autofocus
              bind:value={code}
            />
            {#if method === 'email'}
              <button class="btn" type="button" disabled={busy} onclick={sendEmailCode}
                >{emailSent ? t('sync.resendEmailCode') : t('sync.sendEmailCode')}</button
              >
            {/if}
          </div>
        </Group>
        <div class="settings-buttons">
          <button class="btn" type="button" onclick={reset}>{t('common.cancel')}</button>
          <button class="btn primary" type="submit" disabled={busy || !code.trim()}
            >{t('sync.signIn')}</button
          >
        </div>
      </form>
    {:else if checked?.compatible || status.state === 'signedOut'}
      <form class="sync-form" onsubmit={signIn}>
        {#if status.state === 'signedOut'}
          <p role="alert">{t('sync.signedOutHelp')}</p>
        {/if}
        {#if checked}
          <!-- what the check found: it answers, its version, who may sign up -->
          <Group
            title={t('sync.signInTo', { server: checked.address ?? status.server ?? '' })}
            note={versionNote(checked)}
          >
            <Row label={t('sync.serverState')}>
              <span class="value">{t('sync.serverUp')}, {checked.version}</span>
            </Row>
            {#if checked.options}
              <Row label={t('sync.signUps')}>
                <span class="value"
                  >{checked.options.registration === 'open'
                    ? t('sync.signUpsOpen')
                    : checked.options.registration === 'invite'
                      ? t('sync.signUpsInvite')
                      : t('sync.signUpsClosed')}</span
                >
              </Row>
            {/if}
          </Group>
        {:else}
          <h3 class="sign-in">{t('sync.signInTo', { server: status.server ?? '' })}</h3>
        {/if}
        <Group note={t('sync.passwordHelp')}>
          <div class="field-row">
            <input
              class="field"
              autocomplete="username"
              placeholder={t('sync.username')}
              aria-label={t('sync.username')}
              bind:value={username}
            />
          </div>
          <div class="field-row">
            <PasswordField
              class="field"
              autocomplete="current-password"
              placeholder={t('sync.password')}
              aria-label={t('sync.password')}
              bind:value={password}
            />
          </div>
        </Group>
        {#if plain}
          <p role="alert">{t('sync.plainWarning')}</p>
          <label class="settings-check">
            <input type="checkbox" bind:checked={plainOk} />
            {t('sync.plainConfirm')}
          </label>
        {/if}
        <div class="settings-buttons">
          {#if status.state === 'signedOut'}
            <button class="btn" type="button" onclick={() => (confirming = true)}
              >{t('sync.disconnect')}</button
            >
          {:else}
            <button class="btn" type="button" onclick={reset}>{t('common.back')}</button>
          {/if}
          <button
            class="btn primary"
            type="submit"
            disabled={busy || !username.trim() || !password || (plain && !plainOk)}
            >{busy ? t('sync.signingIn') : t('sync.signIn')}</button
          >
        </div>
        {#if confirming}
          <p class="settings-help">{t('sync.disconnectHelp')}</p>
          <div class="settings-buttons">
            <button class="btn" type="button" onclick={() => (confirming = false)}
              >{t('common.cancel')}</button
            >
            <button class="btn danger" type="button" onclick={disconnect}
              >{t('sync.disconnectConfirm')}</button
            >
          </div>
        {/if}
      </form>
    {:else}
      <form class="sync-form" onsubmit={check}>
        <p class="settings-help">{t('sync.serverDeprecated')}</p>
        <p class="settings-help">{t('sync.intro')}</p>
        <Group>
          <div class="field-row">
            <input
              class="field"
              inputmode="url"
              autocomplete="url"
              placeholder={t('sync.address')}
              aria-label={t('sync.address')}
              bind:value={address}
            />
          </div>
        </Group>
        <div class="settings-buttons">
          <button class="btn primary" type="submit" disabled={busy || !address.trim()}
            >{busy ? t('sync.checking') : t('sync.continue')}</button
          >
        </div>
      </form>
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
{/if}

<style>
  .chevron {
    display: grid;
    color: var(--color-text-faint);
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .chevron.open {
    transform: rotate(180deg);
  }
  .sync-form {
    display: grid;
    gap: var(--space-3);
  }
  .sign-in {
    margin: 0;
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .state {
    display: flex;
    gap: var(--space-3);
    align-items: center;
    padding: var(--space-3);
  }
  .state > div {
    flex: 1;
    min-width: 0;
  }
  .state p {
    margin: 2px 0 0;
  }
  .detail {
    overflow-wrap: anywhere;
  }
  .value {
    color: var(--color-text-muted);
    overflow-wrap: anywhere;
    text-align: right;
  }
  .kind {
    padding: 1px var(--space-2);
    border-radius: var(--radius-chip);
    background: var(--color-surface);
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .error {
    margin: 0;
    color: var(--color-danger);
  }
  .empty {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3);
  }
  .muted {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .start {
    justify-content: flex-start;
  }
  .rename {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
  }
  .rename .field {
    flex: 1;
    min-width: 0;
  }
  .device-state {
    color: var(--color-text-muted);
    text-align: right;
  }
  .device-state.ok {
    color: var(--color-success);
  }
  .warn {
    color: var(--color-warning);
  }
  .device-state.bad {
    color: var(--color-danger);
  }
  .device-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .device-head h3 {
    margin: 0;
    font-size: 16px;
    overflow-wrap: anywhere;
  }
  .notice {
    display: flex;
    gap: var(--space-3);
    max-width: 640px;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-warning);
    border-radius: var(--radius-window);
    background: var(--color-surface-2);
    color: var(--color-warning);
  }
  .notice div {
    display: grid;
    gap: var(--space-1);
    color: var(--color-text);
  }
  .notice p {
    margin: 0;
    color: var(--color-text-muted);
  }
  .mono {
    font-family: var(--font-mono);
  }
</style>
