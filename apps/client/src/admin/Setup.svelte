<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The setup page, opened from the link in the server log. The token in
     the address is traded at once for a setup session and leaves the
     address bar. Then: health checks of the way in (https, the proxy's
     headers, WebSocket, upload size, clock), the admin with a passkey first
     or an authenticator app, its recovery codes, and the first choices made
     with the admin's session: accounts, files, mail, backups. -->
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
    PROTOCOL_VERSION,
    thisDevice,
    type AccountKinds,
    type Registration,
    type Session,
    type TotpSetup,
  } from './api.svelte'
  import Backups from './Backups.svelte'
  import { bytes, errorText } from './format'
  import Policy from './Policy.svelte'
  import { createPasskey, passkeysSupported } from './webauthn'

  interface Status {
    secure: boolean
    passkeys: boolean
    publicUrl: string
  }
  interface Seen {
    secure: boolean
    viaProxy: boolean
    forwardedFor: boolean
    forwardedProto?: string
    clientIp: string
    host: string
    publicUrl: string
    hostMatches: boolean
    serverTime: number
    partBytes: number
    mail: boolean
  }
  type State = 'running' | 'ok' | 'warn' | 'fail'
  interface Check {
    key: 'https' | 'proxy' | 'address' | 'socket' | 'upload' | 'clock'
    state: State
    detail: string
  }
  type Step =
    'checks' | 'use' | 'admin' | 'codes' | 'accounts' | 'files' | 'mail' | 'backups' | 'done'
  /** Sync for one person: sign-up stays closed and the defaults stay, the
   * steps about other people are left out. */
  let personal = $state(true)
  const steps: Step[] = $derived(
    personal
      ? ['checks', 'use', 'admin', 'backups', 'done']
      : ['checks', 'use', 'admin', 'accounts', 'files', 'mail', 'backups', 'done'],
  )
  const SESSION_KEY = 'setup-session'

  /** The token leaves the address before anything else: it works once and
   * should not stay in the history or reach a screenshot. */
  const token = (() => {
    const found = /^#token=(.+)$/.exec(location.hash)
    if (found) history.replaceState(null, '', location.pathname)
    return found?.[1] ? decodeURIComponent(found[1]) : ''
  })()

  function stored(): string {
    try {
      return sessionStorage.getItem(SESSION_KEY) ?? ''
    } catch {
      return ''
    }
  }
  function remember(value: string) {
    try {
      if (value) sessionStorage.setItem(SESSION_KEY, value)
      else sessionStorage.removeItem(SESSION_KEY)
    } catch {
      // without storage the session lasts until a reload
    }
  }

  let status = $state<Status | null>(null)
  let session = $state(stored())
  /** Why the page cannot go on: no link, setup done, plain http. */
  let blocked = $state<'insecure' | 'noLink' | 'done' | ''>('')
  let problem = $state('')
  let busy = $state(false)
  let step = $state<Step>('checks')
  let seen = $state<Seen | null>(null)
  let checks: Check[] = $state([])

  async function begin() {
    problem = ''
    try {
      status = await api<Status>('GET', '/setup')
    } catch (err) {
      if (err instanceof ApiError && err.code === 'setupDone') blocked = 'done'
      else problem = errorText(err)
      return
    }
    if (!status.secure) {
      blocked = 'insecure'
      return
    }
    if (token) {
      try {
        const started = await api<{ session: string }>('POST', '/setup/start', { token })
        session = started.session
        remember(session)
      } catch (err) {
        // a reload after the link was used still has the session
        if (!session) problem = errorText(err)
      }
    }
    if (!session) {
      if (!problem) blocked = 'noLink'
      return
    }
    await runChecks()
  }

  function set(key: Check['key'], state: State, detail = '') {
    const check = checks.find((c) => c.key === key)
    if (check) Object.assign(check, { state, detail })
  }

  async function runChecks() {
    checks = (['https', 'proxy', 'address', 'socket', 'upload', 'clock'] as const).map((key) => ({
      key,
      state: 'running',
      detail: '',
    }))
    const sent = Date.now()
    try {
      seen = await api<Seen>('POST', '/setup/checks', { session })
    } catch (err) {
      return lost(err)
    }
    const middle = (sent + Date.now()) / 2
    const s = seen
    set('https', 'ok', t('admin.setup.check.httpsOk'))
    if (s.viaProxy && s.forwardedFor && s.forwardedProto)
      set('proxy', 'ok', t('admin.setup.check.proxyOk', { ip: s.clientIp }))
    // the loopback peer is a trusted proxy by default: without any
    // forwarded header it is someone on the server itself
    else if (s.viaProxy && (s.forwardedFor || s.forwardedProto))
      set('proxy', 'warn', t('admin.setup.check.proxyHeaders'))
    else set('proxy', 'warn', t('admin.setup.check.proxyNone'))
    if (!s.publicUrl) set('address', 'fail', t('admin.setup.check.addressUnset'))
    else if (!s.hostMatches)
      set(
        'address',
        'warn',
        t('admin.setup.check.addressOther', { url: s.publicUrl, host: s.host }),
      )
    else set('address', 'ok', s.publicUrl)
    const skew = Math.round(Math.abs(s.serverTime - middle) / 1000)
    if (skew > 60) set('clock', 'warn', t('admin.setup.check.clockOff', { seconds: skew }))
    else set('clock', 'ok', t('admin.setup.check.clockOk'))
    await Promise.all([probeSocket(), probeUpload(s.partBytes)])
  }

  function probeSocket(): Promise<void> {
    return new Promise((resolve) => {
      const scheme = location.protocol === 'https:' ? 'wss' : 'ws'
      let settled = false
      const end = (state: State, detail: string) => {
        if (settled) return
        settled = true
        set('socket', state, detail)
        socket.close()
        resolve()
      }
      const socket = new WebSocket(`${scheme}://${location.host}${base}api/v1/setup/socket`)
      socket.onmessage = (event) =>
        end(event.data === 'ok' ? 'ok' : 'fail', t('admin.setup.check.socketOk'))
      socket.onerror = () => end('fail', t('admin.setup.check.socketFailed'))
      setTimeout(() => end('fail', t('admin.setup.check.socketFailed')), 8000)
    })
  }

  async function probeUpload(size: number) {
    const mb = bytes(size)
    try {
      const response = await fetch(`${base}api/v1/setup/upload`, {
        method: 'POST',
        headers: {
          'x-protocol-version': String(PROTOCOL_VERSION),
          'content-type': 'application/octet-stream',
          'x-setup-session': session,
        },
        body: new Uint8Array(size),
      })
      if (response.status === 413)
        set('upload', 'fail', t('admin.setup.check.uploadLimit', { size: mb }))
      else if (!response.ok)
        set('upload', 'fail', t('admin.setup.check.uploadFailed', { status: response.status }))
      else set('upload', 'ok', t('admin.setup.check.uploadOk', { size: mb }))
    } catch {
      set('upload', 'fail', t('admin.setup.check.uploadLimit', { size: mb }))
    }
  }

  /** The session ended (an hour, or too many wrong codes): only a new link
   * from the log goes on. */
  function lost(err: unknown) {
    if (err instanceof ApiError && err.code === 'setupLinkInvalid') {
      session = ''
      remember('')
      blocked = 'noLink'
    } else problem = errorText(err)
  }

  // the admin
  let username = $state('')
  let displayName = $state('')
  let password = $state('')
  let again = $state('')
  const canPasskey = $derived(!!status?.passkeys && passkeysSupported())
  let factor = $state<'passkey' | 'totp' | 'none'>('passkey')
  /** No second step: ticked past the warning. */
  let noneUnderstood = $state(false)
  $effect(() => {
    if (status && !canPasskey) factor = 'totp'
  })
  let totp = $state<TotpSetup | null>(null)
  let totpFor = $state('')
  let code = $state('')
  let codes: string[] = $state([])
  let kept = $state(false)
  /** Backups were set up in the wizard: Continue rather than Skip. */
  let backupsOn = $state(false)
  let copied = $state(false)

  async function showTotp() {
    problem = ''
    if (!username.trim()) {
      problem = t('admin.setup.usernameFirst')
      return
    }
    try {
      totp = await api<TotpSetup>('POST', '/setup/totp', { session, username })
      totpFor = username
    } catch (err) {
      lost(err)
    }
  }

  async function createAdmin(event: SubmitEvent) {
    event.preventDefault()
    problem = ''
    if (password !== again) {
      problem = t('admin.signIn.mismatch')
      return
    }
    if (factor === 'totp' && (!totp || totpFor !== username)) {
      problem = t('admin.setup.scanFirst')
      return
    }
    if (factor === 'none' && !noneUnderstood) {
      problem = t('admin.setup.noneFirst')
      return
    }
    busy = true
    try {
      const display = displayName.trim() || undefined
      let proof: Record<string, unknown>
      if (factor === 'passkey') {
        const options = await api<Record<string, unknown>>('POST', '/setup/passkey', {
          session,
          username,
          displayName: display,
        })
        const credential = await createPasskey(options)
        proof = { type: 'passkey', credential, name: thisDevice().name }
      } else if (factor === 'totp') proof = { type: 'totp', code: code.replace(/\s/g, '') }
      else proof = { type: 'none' }
      const done = await api<{ session: Session; recoveryCodes: string[] }>(
        'POST',
        '/setup/finish',
        {
          session,
          username,
          displayName: display,
          password,
          factor: proof,
          personal,
          device: thisDevice(),
        },
      )
      keep(done.session)
      remember('')
      password = ''
      again = ''
      codes = done.recoveryCodes
      // no second step, no recovery codes to keep
      step = codes.length ? 'codes' : personal ? 'backups' : 'accounts'
    } catch (err) {
      if (err instanceof DOMException) problem = t('admin.signIn.passkeyCancelled')
      else if (err instanceof ApiError && err.code === 'setupLinkInvalid') lost(err)
      else problem = errorText(err)
    } finally {
      busy = false
    }
  }

  async function copyCodes() {
    try {
      await navigator.clipboard.writeText(codes.join('\n'))
      copied = true
      setTimeout(() => (copied = false), 2000)
    } catch {
      problem = t('admin.invites.copyFailed')
    }
  }

  function saveCodes() {
    const text = `${t('admin.account.codesFile', { app: brand.displayName, host: location.host })}\n\n${codes.join('\n')}\n`
    const link = document.createElement('a')
    link.href = URL.createObjectURL(new Blob([text], { type: 'text/plain' }))
    link.download = `${brand.slug}-recovery-codes.txt`
    link.click()
    URL.revokeObjectURL(link.href)
  }

  // the first choices, made with the admin's session
  const MODES: Registration[] = ['closed', 'invite', 'open']
  const KINDS: AccountKinds[] = ['choice', 'e2ee', 'standard']
  let registration: Registration = $state('closed')
  let kinds: AccountKinds = $state('choice')

  async function saveAccounts() {
    busy = true
    problem = ''
    try {
      await api('PUT', '/admin/registration', { registration })
      await api('PUT', '/admin/account-kinds', { accountKinds: kinds })
      step = 'files'
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  let mailTo = $state('')
  let mailState = $state<'sent' | 'failed' | ''>('')
  let mailDetail = $state('')

  async function testMail(event: SubmitEvent) {
    event.preventDefault()
    busy = true
    mailState = ''
    try {
      await api('POST', '/admin/mail/test', { to: mailTo })
      mailState = 'sent'
    } catch (err) {
      mailState = 'failed'
      mailDetail =
        err instanceof ApiError && err.code === 'mailFailed' ? err.message : errorText(err)
    } finally {
      busy = false
    }
  }

  const position = $derived(steps.indexOf(step === 'codes' ? 'admin' : step))
  const httpsFailed = $derived(checks.some((c) => c.key === 'https' && c.state === 'fail'))
  const running = $derived(checks.some((c) => c.state === 'running'))

  // a link pasted into this tab changes only the fragment, nothing reloads
  $effect(() => {
    const again = () => location.reload()
    addEventListener('hashchange', again)
    return () => removeEventListener('hashchange', again)
  })

  void begin()
</script>

<main class="screen">
  <div class="card">
    <h1>{t('admin.setup.title', { app: brand.displayName })}</h1>

    {#if blocked === 'done'}
      <p>{t('admin.setup.done')}</p>
      <a class="btn primary" href="admin">{t('admin.setup.openAdmin')}</a>
    {:else if blocked === 'insecure'}
      <p class="problem" role="alert">{t('admin.setup.insecure')}</p>
      <p class="muted">{t('admin.setup.insecureHelp')}</p>
    {:else if blocked === 'noLink'}
      <p>{t('admin.setup.noLink')}</p>
    {:else if !status && !problem}
      <p class="muted" aria-busy="true">{t('admin.loading')}</p>
    {:else}
      <ol class="steps" aria-label={t('admin.setup.stepsLabel')}>
        {#each steps as name, i (name)}
          <li class:current={i === position} class:past={i < position}>
            {t(`admin.setup.step.${name}`)}
          </li>
        {/each}
      </ol>

      {#if step === 'checks'}
        <p class="muted">{t('admin.setup.checksHelp')}</p>
        <ul class="checks">
          {#each checks as check (check.key)}
            <li class={check.state}>
              <Icon
                name={check.state === 'ok'
                  ? 'check'
                  : check.state === 'running'
                    ? 'syncing'
                    : 'info'}
                size={16}
              />
              <span>
                <b>{t(`admin.setup.check.${check.key}`)}</b>
                {#if check.detail}<span class="muted">{check.detail}</span>{/if}
              </span>
            </li>
          {/each}
        </ul>
        <div class="row">
          <button class="btn" type="button" disabled={running} onclick={runChecks}
            >{t('admin.setup.again')}</button
          >
          <button
            class="btn primary"
            type="button"
            disabled={running || httpsFailed || !seen}
            onclick={() => (step = 'use')}>{t('admin.setup.next')}</button
          >
        </div>
      {:else if step === 'use'}
        <fieldset>
          <legend>{t('admin.setup.useTitle')}</legend>
          {#each [true, false] as one (one)}
            <label class="mode" class:checked={personal === one}>
              <input type="radio" name="use" value={one} bind:group={personal} />
              <span>
                <b>{one ? t('admin.setup.usePersonal') : t('admin.setup.useInstance')}</b>
                <span class="muted"
                  >{one ? t('admin.setup.usePersonalHelp') : t('admin.setup.useInstanceHelp')}</span
                >
              </span>
            </label>
          {/each}
        </fieldset>
        <button class="btn primary" type="button" onclick={() => (step = 'admin')}
          >{t('admin.setup.next')}</button
        >
      {:else if step === 'admin'}
        <form class="form" onsubmit={createAdmin}>
          <p class="muted">
            {t(personal ? 'admin.setup.adminHelpPersonal' : 'admin.setup.adminHelp')}
          </p>
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
            {t('admin.setup.displayName')}
            <input class="field" autocomplete="name" bind:value={displayName} />
          </label>
          <label>
            {t('admin.password')}
            <PasswordField
              class="field"
              autocomplete="new-password"
              required
              bind:value={password}
            />
          </label>
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
          <fieldset>
            <legend>{t('admin.setup.factor')}</legend>
            <label class="mode" class:checked={factor === 'passkey'} class:off={!canPasskey}>
              <input
                type="radio"
                name="factor"
                value="passkey"
                disabled={!canPasskey}
                bind:group={factor}
              />
              <span>
                <b><Icon name="fingerprint" size={16} />{t('admin.setup.passkey')}</b>
                <span class="muted"
                  >{canPasskey
                    ? t('admin.setup.passkeyHelp')
                    : status?.passkeys
                      ? t('admin.setup.passkeyBrowser')
                      : t('admin.setup.passkeyNoAddress')}</span
                >
              </span>
            </label>
            <label class="mode" class:checked={factor === 'totp'}>
              <input type="radio" name="factor" value="totp" bind:group={factor} />
              <span>
                <b><Icon name="timer" size={16} />{t('admin.setup.totp')}</b>
                <span class="muted">{t('admin.setup.totpHelp')}</span>
              </span>
            </label>
            <label class="mode" class:checked={factor === 'none'}>
              <input type="radio" name="factor" value="none" bind:group={factor} />
              <span>
                <b>{t('admin.setup.none')}</b>
                <span class="muted">{t('admin.setup.noneHelp')}</span>
              </span>
            </label>
            {#if factor === 'none'}
              <p class="warn" role="alert">
                {t(personal ? 'admin.setup.noneWarningPersonal' : 'admin.setup.noneWarning')}
              </p>
              <label class="tick">
                <input type="checkbox" bind:checked={noneUnderstood} />
                {t('admin.setup.noneUnderstood')}
              </label>
            {/if}
          </fieldset>
          {#if factor === 'totp'}
            {#if totp && totpFor === username}
              <div class="totp">
                <img
                  class="qr"
                  src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(totp.qrSvg)}`}
                  alt={t('admin.account.qrAlt')}
                  width="180"
                  height="180"
                />
                <div class="form">
                  <p>{t('admin.account.scan')}</p>
                  <p class="muted">{t('admin.account.typeSecret')}</p>
                  <code class="secret">{totp.secret.match(/.{1,4}/g)?.join(' ')}</code>
                  <label>
                    {t('admin.account.firstCode')}
                    <input
                      class="field mono"
                      inputmode="numeric"
                      autocomplete="one-time-code"
                      maxlength="7"
                      required
                      bind:value={code}
                    />
                  </label>
                </div>
              </div>
            {:else}
              <button class="btn" type="button" onclick={showTotp}
                >{t('admin.setup.showTotp')}</button
              >
            {/if}
          {/if}
          {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
          <button class="btn primary" type="submit" disabled={busy}
            >{t(personal ? 'admin.setup.createPersonal' : 'admin.setup.create')}</button
          >
        </form>
      {:else if step === 'codes'}
        <p>{t('admin.setup.codesHelp')}</p>
        <ol class="codes">
          {#each codes as one (one)}<li><code>{one}</code></li>{/each}
        </ol>
        <div class="row">
          <button class="btn" type="button" onclick={copyCodes}
            >{copied ? t('admin.setup.copied') : t('admin.setup.copy')}</button
          >
          <button class="btn" type="button" onclick={saveCodes}>{t('admin.setup.save')}</button>
        </div>
        <label class="tick">
          <input type="checkbox" bind:checked={kept} />
          {t('admin.setup.kept')}
        </label>
        <button
          class="btn primary"
          type="button"
          disabled={!kept}
          onclick={() => (step = personal ? 'backups' : 'accounts')}>{t('admin.setup.next')}</button
        >
      {:else if step === 'accounts'}
        <fieldset>
          <legend>{t('admin.overview.registration')}</legend>
          {#each MODES as mode (mode)}
            <label class="mode" class:checked={registration === mode}>
              <input type="radio" name="registration" value={mode} bind:group={registration} />
              <span>
                <b>{t(`admin.registration.${mode}`)}</b>
                <span class="muted">{t(`admin.registration.${mode}Help`)}</span>
              </span>
            </label>
          {/each}
          {#if registration === 'open'}
            <p class="warn" role="alert">{t('admin.registration.openRisks')}</p>
          {/if}
        </fieldset>
        <fieldset>
          <legend>{t('admin.overview.accountKinds')}</legend>
          {#each KINDS as one (one)}
            <label class="mode" class:checked={kinds === one}>
              <input type="radio" name="account-kinds" value={one} bind:group={kinds} />
              <span>
                <b>{t(`admin.accountKinds.${one}`)}</b>
                <span class="muted">{t(`admin.accountKinds.${one}Help`)}</span>
              </span>
            </label>
          {/each}
        </fieldset>
        {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
        <button class="btn primary" type="button" disabled={busy} onclick={saveAccounts}
          >{t('admin.setup.next')}</button
        >
      {:else if step === 'files'}
        <p class="muted">{t('admin.setup.filesHelp')}</p>
        <Policy path="/admin/policy" level="instance" />
        <button class="btn primary" type="button" onclick={() => (step = 'mail')}
          >{t('admin.setup.next')}</button
        >
      {:else if step === 'mail'}
        {#if seen?.mail}
          <form class="form" onsubmit={testMail}>
            <p class="muted">{t('admin.setup.mailHelp')}</p>
            <label>
              {t('admin.setup.mailTo')}
              <input class="field" type="email" autocomplete="email" required bind:value={mailTo} />
            </label>
            <button class="btn" type="submit" disabled={busy}>{t('admin.setup.mailSend')}</button>
            {#if mailState === 'sent'}
              <p class="ok" role="status">{t('admin.setup.mailSent', { to: mailTo })}</p>
            {:else if mailState === 'failed'}
              <p class="problem" role="alert">{t('admin.setup.mailFailed', { why: mailDetail })}</p>
            {/if}
          </form>
        {:else}
          <p class="muted">{t('admin.setup.mailOff')}</p>
        {/if}
        <button class="btn primary" type="button" onclick={() => (step = 'backups')}
          >{t('admin.setup.next')}</button
        >
      {:else if step === 'backups'}
        <p class="muted">{t('admin.setup.backupsHelp')}</p>
        <Backups onconfigured={(on) => (backupsOn = on)} />
        {#if backupsOn}
          <button class="btn primary" type="button" onclick={() => (step = 'done')}
            >{t('admin.setup.next')}</button
          >
        {:else}
          <p class="muted">{t('admin.setup.backupsLater')}</p>
          <button class="btn" type="button" onclick={() => (step = 'done')} data-backups-skip
            >{t('admin.setup.skip')}</button
          >
        {/if}
      {:else}
        <p>
          {personal
            ? t('admin.setup.finishedPersonal', {
                app: brand.displayName,
                address: location.origin + location.pathname.replace(/\/setup\/?$/, ''),
              })
            : t('admin.setup.finished')}
        </p>
        <p class="warn">{t('admin.setup.secrets')}</p>
        <a class="btn primary" href="admin">{t('admin.setup.openAdmin')}</a>
      {/if}
      {#if problem && step === 'checks'}<p class="problem" role="alert">{problem}</p>{/if}
    {/if}
    {#if problem && !status}
      <p class="problem" role="alert">{problem}</p>
      <button class="btn" type="button" onclick={begin}>{t('admin.retry')}</button>
    {/if}
  </div>
</main>

<style>
  .screen {
    min-height: 100%;
    display: grid;
    place-items: start center;
    padding: var(--space-5) var(--space-4);
  }
  .card {
    width: min(640px, 100%);
    display: grid;
    gap: var(--space-4);
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
  .steps {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 13px;
    color: var(--color-text-muted);
  }
  .steps li {
    padding: 2px var(--space-2);
    border-radius: var(--radius-control);
  }
  .steps li.past {
    color: var(--color-text);
  }
  .steps li.current {
    color: var(--color-accent-text);
    font-weight: 600;
    box-shadow: inset 0 0 0 1px var(--color-accent);
  }
  .checks {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .checks li {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-left-width: 3px;
    border-radius: var(--radius-popover);
  }
  .checks li span {
    display: grid;
    gap: 2px;
  }
  .checks li.ok {
    border-left-color: var(--color-success);
  }
  .checks li.warn {
    border-left-color: var(--color-warning);
  }
  .checks li.fail {
    border-left-color: var(--color-danger);
  }
  .form {
    display: grid;
    gap: var(--space-3);
  }
  label {
    display: grid;
    gap: var(--space-1);
  }
  fieldset {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: var(--space-2);
    font-weight: 600;
  }
  .mode {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    cursor: pointer;
  }
  .mode.checked {
    border-color: var(--color-accent);
  }
  .mode.off {
    opacity: 0.6;
    cursor: default;
  }
  .mode span {
    display: grid;
    gap: 2px;
  }
  .mode b {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .totp {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    align-items: start;
  }
  .totp .form {
    flex: 1;
    min-width: 220px;
  }
  .qr {
    border-radius: var(--radius-control);
    background: #fff;
  }
  .secret {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }
  .codes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: var(--space-2);
    margin: 0;
    padding-left: var(--space-5);
    font-family: var(--font-mono);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .tick {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .warn {
    padding: var(--space-2) var(--space-3);
    border-left: 3px solid var(--color-accent);
  }
  fieldset .warn {
    margin: var(--space-2) 0 0;
    border-left-color: var(--color-danger);
  }
  .card a.btn {
    justify-self: start;
    text-decoration: none;
  }
  .problem {
    color: var(--color-danger);
  }
  .ok {
    color: var(--color-success);
  }
</style>
