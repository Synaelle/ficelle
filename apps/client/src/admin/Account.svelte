<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The signed in admin's own sign-in: authenticator app, recovery codes,
     passkeys and single sign-on. Changes ask for the password again. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import { brand } from '@app/brand'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { overlay } from '../lib/overlay.svelte'
  import {
    api,
    base,
    auth,
    flash,
    keep,
    signOut,
    type PrivacyInfo,
    type AuthOptions,
    type PasskeyCeremony,
    type Security,
    type TotpSetup,
    type User,
  } from './api.svelte'
  import { proofOf } from './e2ee'
  import { date, dateTime, errorText, relative } from './format'
  import { createPasskey, passkeysSupported } from './webauthn'

  let security: Security | null = $state(null)
  let options: AuthOptions | null = $state(null)
  let problem = $state('')
  let notice = $state(flash.account)
  flash.account = ''

  /** Authenticator app being set up. */
  let setup: TotpSetup | null = $state(null)
  let firstCode = $state('')
  let setupPassword = $state('')
  /** Recovery codes just made, shown once. */
  let codes: string[] = $state([])
  let copied = $state(false)

  async function run(action: () => Promise<unknown>, done = '') {
    problem = ''
    if (done) notice = ''
    try {
      await action()
      if (done) notice = done
      return true
    } catch (err) {
      problem = err instanceof DOMException ? t('admin.signIn.passkeyCancelled') : errorText(err)
      return false
    }
  }

  async function load() {
    await run(async () => {
      ;[security, options, privacy] = await Promise.all([
        api<Security>('GET', '/me/security'),
        api<AuthOptions>('GET', '/auth/options'),
        api<PrivacyInfo>('GET', '/privacy'),
      ])
      // a second factor just added lifts the wait of a command line admin,
      // the panel opens once the session knows
      if (auth.session?.user.factorRequired) {
        const user = await api<User>('GET', '/me')
        if (!user.factorRequired && auth.session) keep({ ...auth.session, user })
      }
    })
  }

  let privacy: PrivacyInfo | null = $state(null)

  async function setEmail() {
    const email = await overlay.askText(t('admin.account.emailPrompt'), security?.email ?? '')
    if (!email?.trim()) return
    const confirm = await proof()
    if (!confirm) return
    await run(
      () => api('PUT', '/me/email', { email, ...confirm }),
      t('admin.account.emailSent', { email: email.trim() }),
    )
  }

  async function removeEmail() {
    const confirm = await proof()
    if (!confirm) return
    if (await run(() => api('DELETE', '/me/email', confirm), t('admin.account.removed')))
      await load()
  }

  async function exportData() {
    await run(async () => {
      const data = await api<unknown>('GET', '/me/export')
      const link = document.createElement('a')
      link.href = URL.createObjectURL(
        new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' }),
      )
      link.download = `${brand.slug}-${auth.session?.user.username ?? 'account'}.json`
      link.click()
      URL.revokeObjectURL(link.href)
    }, t('admin.account.exported'))
  }

  async function requestDeletion() {
    const days = privacy?.deletionGraceDays ?? 0
    const yes = await overlay.ask(
      days > 0 ? t('admin.account.deleteConfirm', { days }) : t('admin.account.deleteNowConfirm'),
      { confirmLabel: t('admin.account.delete'), danger: true },
    )
    if (!yes) return
    const confirm = await proof()
    if (!confirm) return
    await run(async () => {
      await api('POST', '/me/deletion', confirm)
      if (days === 0) await signOut()
      else await load()
    }, t('admin.account.deletionPlanned'))
  }

  async function cancelDeletion() {
    if (await run(() => api('DELETE', '/me/deletion'), t('admin.account.deletionCancelled')))
      await load()
  }

  const e2ee = $derived(auth.session?.user.kind === 'e2ee')

  /** What proves the password: the password itself, or for an e2ee account
   * a proof from an OPAQUE run, the password never leaving the page. */
  async function confirmWith(password: string): Promise<{ password?: string; proof?: string }> {
    if (!e2ee) return { password }
    return { proof: await proofOf(auth.session?.user.username ?? '', password) }
  }

  /** The password, asked again. Null when the person cancels or it is
   * wrong. An account without one proves itself by a recent sign-in, it
   * sends nothing. */
  async function proof(): Promise<{ password?: string; proof?: string } | null> {
    if (!security?.hasPassword) return {}
    const password = await overlay.askPassword(t('admin.account.confirmPrompt'))
    if (!password) return null
    try {
      return await confirmWith(password)
    } catch (err) {
      problem = errorText(err)
      return null
    }
  }

  async function startTotp() {
    await run(async () => {
      setup = await api<TotpSetup>('POST', '/me/totp/setup')
      firstCode = ''
      setupPassword = ''
    })
  }

  async function confirmTotp(event: SubmitEvent) {
    event.preventDefault()
    await run(async () => {
      const answer = await api<{ codes: string[] }>('POST', '/me/totp/confirm', {
        code: firstCode,
        ...(security?.hasPassword ? await confirmWith(setupPassword) : {}),
      })
      setup = null
      codes = answer.codes
      notice = t('admin.account.totpOn')
      await load()
    })
  }

  async function disableTotp() {
    const yes = await overlay.ask(t('admin.account.totpOffConfirm'), {
      confirmLabel: t('admin.account.turnOff'),
      danger: true,
    })
    if (!yes) return
    const confirm = await proof()
    if (!confirm) return
    if (await run(() => api('POST', '/me/totp/disable', confirm), t('admin.account.totpOff')))
      await load()
  }

  async function enableEmailCodes() {
    if (await run(() => api('POST', '/me/email-codes/enable'), t('admin.account.emailCodesOn')))
      await load()
  }

  async function disableEmailCodes() {
    const confirm = await proof()
    if (!confirm) return
    if (
      await run(
        () => api('POST', '/me/email-codes/disable', confirm),
        t('admin.account.emailCodesOff'),
      )
    )
      await load()
  }

  async function newCodes() {
    const confirm = await proof()
    if (!confirm) return
    await run(async () => {
      codes = (await api<{ codes: string[] }>('POST', '/me/recovery-codes', confirm)).codes
      await load()
    }, t('admin.account.codesReady'))
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

  async function addPasskey() {
    const name = await overlay.askText(t('admin.account.passkeyName'), defaultPasskeyName())
    if (name === null) return
    const confirm = await proof()
    if (!confirm) return
    await run(async () => {
      const ceremony = await api<PasskeyCeremony>('POST', '/me/passkeys/start', confirm)
      const credential = await createPasskey(ceremony.options)
      const added = await api<{ codes: string[] }>('POST', '/me/passkeys/finish', {
        id: ceremony.id,
        credential,
        name,
      })
      if (added.codes.length) codes = added.codes
      await load()
    }, t('admin.account.passkeyAdded'))
  }

  function defaultPasskeyName() {
    const ua = navigator.userAgent
    if (/android/i.test(ua)) return 'Android'
    if (/iphone|ipad/i.test(ua)) return 'iPhone'
    if (/mac os/i.test(ua)) return 'Mac'
    if (/windows/i.test(ua)) return 'Windows'
    return t('admin.account.passkey')
  }

  async function removePasskey(id: string, name: string) {
    const yes = await overlay.ask(t('admin.account.passkeyRemoveConfirm', { name }), {
      confirmLabel: t('admin.account.remove'),
      danger: true,
    })
    if (!yes) return
    const confirm = await proof()
    if (!confirm) return
    if (await run(() => api('DELETE', `/me/passkeys/${id}`, confirm), t('admin.account.removed')))
      await load()
  }

  async function linkSso() {
    const confirm = await proof()
    if (!confirm) return
    await run(async () => {
      const { url } = await api<{ url: string }>('POST', '/me/sso/link', {
        returnTo: `${base}admin`,
        ...confirm,
      })
      location.assign(url)
    })
  }

  async function unlinkSso(id: string, name: string) {
    const yes = await overlay.ask(t('admin.account.ssoUnlinkConfirm', { name }), {
      confirmLabel: t('admin.account.remove'),
      danger: true,
    })
    if (!yes) return
    const confirm = await proof()
    if (!confirm) return
    if (await run(() => api('DELETE', `/me/sso/${id}`, confirm), t('admin.account.removed')))
      await load()
  }

  void load()
</script>

<p class="muted lead">{t('admin.account.help')}</p>
{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
{#if notice}<p class="notice" role="status">{notice}</p>{/if}
{#if security?.deleteAt}
  <div class="pending" role="status">
    <p>{t('admin.account.deletionPending', { date: dateTime(security.deleteAt) })}</p>
    <button class="btn primary" onclick={cancelDeletion}>{t('admin.account.keepAccount')}</button>
  </div>
{/if}

{#if codes.length}
  <section class="fresh" aria-labelledby="codes-title">
    <h2 id="codes-title">{t('admin.account.codesTitle')}</h2>
    <p>{t('admin.account.codesHelp')}</p>
    <ul class="codes">
      {#each codes as code (code)}<li><code>{code}</code></li>{/each}
    </ul>
    <div class="actions">
      <button class="btn" onclick={copyCodes}
        ><Icon name={copied ? 'check' : 'copy'} size={16} />{copied
          ? t('admin.invites.copied')
          : t('admin.invites.copy')}</button
      >
      <button class="btn" onclick={saveCodes}
        ><Icon name="export" size={16} />{t('admin.account.saveTxt')}</button
      >
      <button class="btn primary" onclick={() => (codes = [])}
        >{t('admin.account.codesSaved')}</button
      >
    </div>
  </section>
{/if}

{#snippet head(icon: IconName, title: string, state: string, on = false)}
  <summary>
    <Icon name={icon} size={18} />
    <b>{title}</b>
    <span class="state" class:on>{state}</span>
    <Icon name="right" size={16} />
  </summary>
{/snippet}

{#if security}
  <h2 class="group">{t('admin.account.signInGroup')}</h2>
  <div class="list">
    <details class="item" open={!!setup}>
      {@render head(
        'timer',
        t('admin.account.totpTitle'),
        security.totp ? t('admin.account.on') : t('admin.account.off'),
        security.totp,
      )}
      <div class="body">
        <p class="muted">{t('admin.account.totpHelp')}</p>
        {#if security.totp}
          <button class="btn danger" onclick={disableTotp}>{t('admin.account.turnOff')}</button>
        {:else if setup}
          <form class="setup" onsubmit={confirmTotp}>
            <img
              class="qr"
              src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(setup.qrSvg)}`}
              alt={t('admin.account.qrAlt')}
              width="200"
              height="200"
            />
            <div class="steps">
              <p>{t('admin.account.scan')}</p>
              <p class="muted">{t('admin.account.typeSecret')}</p>
              <code class="secret">{setup.secret.match(/.{1,4}/g)?.join(' ')}</code>
              <label>
                {t('admin.account.firstCode')}
                <input
                  class="field mono"
                  inputmode="numeric"
                  autocomplete="one-time-code"
                  maxlength="7"
                  required
                  bind:value={firstCode}
                />
              </label>
              {#if security.hasPassword}
                <label>
                  {t('admin.account.yourPassword')}
                  <PasswordField
                    class="field"
                    autocomplete="current-password"
                    required
                    bind:value={setupPassword}
                  />
                </label>
              {/if}
              <div class="actions">
                <button class="btn" type="button" onclick={() => (setup = null)}
                  >{t('common.cancel')}</button
                >
                <button class="btn primary" type="submit">{t('admin.account.turnOn')}</button>
              </div>
            </div>
          </form>
        {:else}
          <button class="btn primary" onclick={startTotp}>{t('admin.account.setUp')}</button>
        {/if}
      </div>
    </details>

    <details class="item">
      {@render head(
        'fingerprint',
        t('admin.account.passkeysTitle'),
        !options?.passkeys
          ? t('admin.account.unavailable')
          : security.passkeys.length
            ? t('admin.account.passkeyCount', { count: security.passkeys.length })
            : t('admin.account.off'),
        security.passkeys.length > 0,
      )}
      <div class="body">
        <p class="muted">{t('admin.account.passkeysHelp')}</p>
        {#if !options?.passkeys}
          <p class="muted">{t('admin.account.passkeysOff')}</p>
        {:else}
          {#if security.passkeys.length}
            <ul class="items">
              {#each security.passkeys as passkey (passkey.id)}
                <li>
                  <span
                    ><b>{passkey.name}</b><span class="muted"
                      >{t('admin.account.addedOn', { date: date(passkey.createdAt) })}, {t(
                        'admin.account.lastUsed',
                        { when: relative(passkey.lastUsedAt) },
                      )}</span
                    ></span
                  >
                  <button class="btn" onclick={() => removePasskey(passkey.id, passkey.name)}
                    >{t('admin.account.remove')}</button
                  >
                </li>
              {/each}
            </ul>
          {/if}
          <button class="btn" onclick={addPasskey} disabled={!passkeysSupported()}
            ><Icon name="plus" size={16} />{t('admin.account.addPasskey')}</button
          >
        {/if}
      </div>
    </details>

    {#if security.emailAvailable}
      <details class="item">
        {@render head(
          'invite',
          t('admin.account.emailCodesTitle'),
          security.emailCodes ? t('admin.account.on') : t('admin.account.off'),
          security.emailCodes,
        )}
        <div class="body">
          <p class="muted">{t('admin.account.emailCodesHelp')}</p>
          {#if security.emailCodes}
            <p>{t('admin.account.emailCodesActive', { email: security.email ?? '' })}</p>
            <button class="btn" onclick={disableEmailCodes}>{t('admin.account.turnOff')}</button>
          {:else if security.email}
            <button class="btn" onclick={enableEmailCodes}
              >{t('admin.account.emailCodesEnable')}</button
            >
          {:else}
            <p class="muted">{t('admin.account.emailCodesNeedsEmail')}</p>
          {/if}
        </div>
      </details>
    {/if}

    {#if security.totp || security.passkeys.length || security.emailCodes}
      <details class="item">
        {@render head(
          'key',
          t('admin.account.recoveryTitle'),
          t('admin.account.codesLeftShort', { count: security.recoveryCodesLeft }),
          security.recoveryCodesLeft > 0,
        )}
        <div class="body">
          <p class="muted">
            {t('admin.account.recoveryHelp')}
            {t('admin.account.codesLeft', { count: security.recoveryCodesLeft })}
          </p>
          <button class="btn" onclick={newCodes}>{t('admin.account.newCodes')}</button>
        </div>
      </details>
    {/if}

    {#if options?.sso}
      <details class="item">
        {@render head(
          'link',
          t('admin.account.ssoTitle'),
          security.sso.length
            ? t('admin.account.linkedCount', { count: security.sso.length })
            : t('admin.account.off'),
          security.sso.length > 0,
        )}
        <div class="body">
          <p class="muted">{t('admin.account.ssoHelp', { provider: options.sso })}</p>
          {#if security.sso.length}
            <ul class="items">
              {#each security.sso as identity (identity.id)}
                <li>
                  <span
                    ><b>{identity.name}</b><span class="muted"
                      >{t('admin.account.addedOn', { date: date(identity.createdAt) })}</span
                    ></span
                  >
                  <button class="btn" onclick={() => unlinkSso(identity.id, identity.name)}
                    >{t('admin.account.remove')}</button
                  >
                </li>
              {/each}
            </ul>
          {/if}
          <button class="btn" onclick={linkSso}
            ><Icon name="plus" size={16} />{t('admin.account.linkSso', {
              provider: options.sso,
            })}</button
          >
        </div>
      </details>
    {/if}
  </div>

  <h2 class="group">{t('admin.account.accountGroup')}</h2>
  <div class="list">
    <details class="item">
      {@render head(
        'invite',
        t('admin.account.emailTitle'),
        !security.emailAvailable
          ? t('admin.account.unavailable')
          : (security.email ?? t('admin.account.off')),
        !!security.email,
      )}
      <div class="body">
        {#if !security.emailAvailable}
          <p class="muted">{t('admin.account.emailOff')}</p>
        {:else}
          <p class="muted">{t('admin.account.emailHelp')}</p>
          <div class="row">
            <button class="btn" onclick={setEmail}
              >{security.email
                ? t('admin.account.changeEmail')
                : t('admin.account.addEmail')}</button
            >
            {#if security.email}
              <button class="btn" onclick={removeEmail}>{t('admin.account.remove')}</button>
            {/if}
          </div>
        {/if}
      </div>
    </details>

    <details class="item">
      {@render head('database', t('admin.account.dataTitle'), '')}
      <div class="body">
        <p class="muted">{t('admin.account.dataHelp')}</p>
        <div class="row">
          <button class="btn" onclick={exportData}
            ><Icon name="export" size={16} />{t('admin.account.export')}</button
          >
          {#if !security.deleteAt}
            <button class="btn danger" onclick={requestDeletion}>{t('admin.account.delete')}</button
            >
          {/if}
          <a class="privacy-link" href="privacy">{t('admin.privacy.link')}</a>
        </div>
      </div>
    </details>
  </div>
{/if}

<style>
  .lead {
    max-width: 620px;
    margin: 0 0 var(--space-4);
  }
  .group {
    margin: var(--space-5) 0 var(--space-2);
    color: var(--color-text-muted);
    font-size: 13px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .list {
    max-width: 680px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
  }
  .item + .item {
    border-top: 1px solid var(--color-border);
  }
  .item summary {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    cursor: pointer;
    list-style: none;
  }
  .item summary::-webkit-details-marker {
    display: none;
  }
  .item summary:hover {
    background: var(--color-surface);
  }
  .item summary b {
    flex: 1;
    font-weight: 500;
  }
  .state {
    color: var(--color-text-muted);
    font-size: 13px;
    overflow-wrap: anywhere;
    text-align: end;
  }
  .state.on {
    color: var(--color-text);
  }
  .item summary > :global(svg:last-child) {
    color: var(--color-text-muted);
    transition: transform 0.15s;
  }
  .item[open] summary > :global(svg:last-child) {
    transform: rotate(90deg);
  }
  .body {
    display: grid;
    gap: var(--space-3);
    justify-items: start;
    padding: 0 var(--space-4) var(--space-4) calc(var(--space-4) + 18px + var(--space-3));
  }
  .fresh {
    display: grid;
    gap: var(--space-2);
    justify-items: start;
    max-width: 680px;
    margin-bottom: var(--space-5);
  }
  .fresh {
    padding: var(--space-4);
    border: 1px solid var(--color-accent);
    border-radius: var(--radius-popover);
  }
  h2 {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    font-size: 17px;
  }
  p {
    margin: 0;
  }
  .row,
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .setup {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    align-items: start;
  }
  .qr {
    border-radius: var(--radius-control);
    background: #fff;
  }
  .steps {
    display: grid;
    gap: var(--space-2);
    min-width: 240px;
    flex: 1;
  }
  label {
    display: grid;
    gap: var(--space-1);
  }
  .mono,
  code {
    font-family: var(--font-mono);
  }
  .secret {
    overflow-wrap: anywhere;
    user-select: all;
    letter-spacing: 0.05em;
  }
  .codes {
    display: grid;
    grid-template-columns: repeat(2, max-content);
    gap: var(--space-1) var(--space-5);
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 15px;
  }
  .items {
    display: grid;
    gap: var(--space-2);
    width: 100%;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .items li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border);
  }
  .items li > span {
    display: grid;
  }
  .problem {
    color: var(--color-danger);
  }
  .pending {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    max-width: 680px;
    margin-bottom: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-danger);
    border-radius: var(--radius-popover);
  }
  .privacy-link {
    color: var(--color-accent-text);
  }
  .notice {
    color: var(--color-accent-text);
    margin-bottom: var(--space-3);
  }
  @media (max-width: 480px) {
    .codes {
      grid-template-columns: 1fr;
    }
  }
</style>
