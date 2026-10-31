<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Pages anyone reaches without signing in: the link that confirms an
     email address, the link that sets a new password, a new password with
     the recovery code of an end-to-end encrypted account, and the privacy
     page, written from the server's own settings. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import { brand } from '@app/brand'
  import { onMount } from 'svelte'
  import { t } from '../lib/i18n.svelte'
  import { api, keep, signOut, thisDevice, type PrivacyInfo } from './api.svelte'
  import { recover } from './e2ee'
  import { errorText } from './format'

  let { page, token = '' }: { page: 'verify' | 'reset' | 'recover' | 'privacy'; token?: string } =
    $props()

  let busy = $state(false)
  let problem = $state('')
  let done = $state('')

  let username = $state('')
  let code = $state('')
  let password = $state('')
  let again = $state('')

  let privacy: PrivacyInfo | null = $state(null)
  const host = location.host

  async function attempt(action: () => Promise<void>) {
    problem = ''
    busy = true
    try {
      await action()
    } catch (err) {
      problem =
        err instanceof Error && !('code' in err) ? t('admin.recover.codeInvalid') : errorText(err)
    } finally {
      busy = false
    }
  }

  function samePasswords() {
    if (password === again) return true
    problem = t('admin.signIn.mismatch')
    return false
  }

  async function resetPassword(event: SubmitEvent) {
    event.preventDefault()
    if (!samePasswords()) return
    await attempt(async () => {
      await api('POST', '/auth/reset/finish', { token, password })
      done = t('admin.reset.done')
    })
  }

  async function recoverAccount(event: SubmitEvent) {
    event.preventDefault()
    if (!samePasswords()) return
    await attempt(async () => {
      keep(await recover(username, code, password, thisDevice()))
      // the account is for the app, this page only set its password
      await signOut()
      done = t('admin.recover.done')
    })
  }

  async function verify() {
    await attempt(async () => {
      const answer = await api<{ username: string; email: string }>('POST', '/auth/email/verify', {
        token,
      })
      done = t('admin.verify.done', { email: answer.email, name: answer.username })
    })
  }

  async function loadPrivacy() {
    await attempt(async () => {
      privacy = await api<PrivacyInfo>('GET', '/privacy')
    })
  }

  onMount(() => {
    if (page === 'verify') void verify()
    if (page === 'privacy') void loadPrivacy()
  })
</script>

{#if page === 'privacy'}
  <main class="page">
    <article class="privacy">
      <h1>{t('admin.privacy.title', { host })}</h1>
      {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
      {#if privacy}
        <h2>{t('admin.privacy.whoTitle')}</h2>
        {#if privacy.operator}
          <p>
            {t('admin.privacy.operator', { operator: privacy.operator, app: brand.displayName })}
          </p>
        {:else}
          <p>{t('admin.privacy.noOperator', { app: brand.displayName })}</p>
        {/if}
        {#if privacy.contact}<p>{t('admin.privacy.contact', { contact: privacy.contact })}</p>{/if}

        <h2>{t('admin.privacy.keptTitle')}</h2>
        <ul>
          <li>{t('admin.privacy.keptAccount')}</li>
          <li>{t('admin.privacy.keptDevices', { days: privacy.sessionDays })}</li>
          <li>{t('admin.privacy.keptLog', { days: privacy.auditDays })}</li>
          {#if privacy.accountKinds !== 'e2ee'}<li>{t('admin.privacy.keptStandard')}</li>{/if}
          {#if privacy.accountKinds !== 'standard'}<li>{t('admin.privacy.keptE2ee')}</li>{/if}
        </ul>

        <h2>{t('admin.privacy.notTitle')}</h2>
        <p>{t('admin.privacy.notKept')}</p>

        {#if privacy.feedbackReceive || privacy.feedbackSendTo !== 'off'}
          <h2>{t('admin.privacy.feedbackTitle')}</h2>
          <ul>
            {#if privacy.feedbackReceive}
              <li>{t('admin.privacy.feedbackKept', { days: privacy.feedbackKeepDays })}</li>
            {/if}
            {#if privacy.feedbackSendTo === 'main'}
              <li>{t('admin.privacy.feedbackMain', { host: privacy.feedbackMain })}</li>
            {:else if privacy.feedbackSendTo === 'here'}
              <li>{t('admin.privacy.feedbackHere')}</li>
            {/if}
          </ul>
        {/if}

        {#if privacy.email || privacy.sso}
          <h2>{t('admin.privacy.othersTitle')}</h2>
          <ul>
            {#if privacy.email}<li>{t('admin.privacy.email')}</li>{/if}
            {#if privacy.sso}<li>{t('admin.privacy.sso', { provider: privacy.sso })}</li>{/if}
          </ul>
        {/if}

        <h2>{t('admin.privacy.rightsTitle')}</h2>
        <ul>
          <li>{t('admin.privacy.export')}</li>
          <li>
            {privacy.deletionGraceDays > 0
              ? t('admin.privacy.deletion', { days: privacy.deletionGraceDays })
              : t('admin.privacy.deletionNow')}
          </li>
          <li>{t('admin.privacy.correct')}</li>
        </ul>

        {#if privacy.notice}
          <h2>{t('admin.privacy.noticeTitle')}</h2>
          <p class="notice-text">{privacy.notice}</p>
        {/if}
      {/if}
      <p><a href="admin">{t('admin.public.back')}</a></p>
    </article>
  </main>
{:else}
  <main class="screen">
    <div class="card">
      {#if page === 'verify'}
        <h1>{t('admin.verify.title')}</h1>
        {#if busy}<p class="muted" aria-busy="true">{t('admin.loading')}</p>{/if}
      {:else if page === 'reset'}
        <h1>{t('admin.reset.title')}</h1>
      {:else}
        <h1>{t('admin.recover.title')}</h1>
      {/if}

      {#if done}
        <p role="status">{done}</p>
        <a class="btn" href="admin">{t('admin.public.back')}</a>
      {:else if page === 'reset'}
        <form onsubmit={resetPassword}>
          <label>
            {t('admin.reset.password')}
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
          {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
          <button class="btn primary" type="submit" disabled={busy}
            >{t('admin.reset.submit')}</button
          >
        </form>
      {:else if page === 'recover'}
        <p class="muted">{t('admin.recover.help')}</p>
        <form onsubmit={recoverAccount}>
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
            {t('admin.recover.code')}
            <input
              class="field mono"
              autocomplete="off"
              spellcheck="false"
              required
              bind:value={code}
            />
          </label>
          <label>
            {t('admin.reset.password')}
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
          {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
          <button class="btn primary" type="submit" disabled={busy}
            >{t('admin.recover.submit')}</button
          >
        </form>
      {:else if problem}
        <p class="problem" role="alert">{problem}</p>
        <a class="btn" href="admin">{t('admin.public.back')}</a>
      {/if}
    </div>
  </main>
{/if}

<style>
  .screen {
    min-height: 100%;
    display: grid;
    place-items: center;
    padding: var(--space-4);
  }
  .card,
  form {
    display: grid;
    gap: var(--space-3);
  }
  .card {
    width: min(400px, 100%);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  .page {
    min-height: 100%;
    padding: var(--space-5) var(--space-4) var(--space-6);
  }
  .privacy {
    max-width: 680px;
    margin: 0 auto;
    line-height: 1.55;
  }
  h1 {
    margin: 0;
    font-size: 22px;
  }
  .privacy h1 {
    margin-bottom: var(--space-4);
  }
  h2 {
    margin: var(--space-5) 0 var(--space-2);
    font-size: 17px;
  }
  p {
    margin: 0;
  }
  .privacy p,
  .privacy ul {
    margin: 0 0 var(--space-2);
  }
  .privacy li {
    margin-bottom: var(--space-1);
  }
  .notice-text {
    white-space: pre-wrap;
  }
  label {
    display: grid;
    gap: var(--space-1);
  }
  .mono {
    font-family: var(--font-mono);
    letter-spacing: 0.04em;
  }
  .problem {
    color: var(--color-danger);
  }
  a.btn {
    justify-content: center;
    text-decoration: none;
  }
  a:not(.btn) {
    color: var(--color-accent-text);
  }
</style>
