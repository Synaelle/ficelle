<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The report of an error nothing handled (lib/errorCatch): the error and
     recent logs filled in and cleaned as in the feedback form, a line on
     what the person was doing, a contact, and the screenshot taken as Report
     it was pressed, left out unless ticked. Where reports are turned off,
     the contact address instead. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { onMount } from 'svelte'
  import { backend, type FeedbackWhere } from './backend'
  import { errorCatch } from './errorCatch.svelte'
  import {
    contactOk,
    installId,
    MAX_ABOUT,
    MAX_CONTACT,
    MAX_MESSAGE,
    newReceipt,
    prepareImage,
    rememberSent,
    reportLogs,
    scrub,
  } from './feedback'
  import Icon from './icons/Icon.svelte'
  import { t } from './i18n.svelte'
  import { modal } from './modal'
  import { workspace } from './workspace.svelte'

  let place: FeedbackWhere | null = $state(null)
  let doing = $state('')
  let contact = $state('')
  let withShot = $state(false)
  let shotUrl = $state('')
  let busy = $state(false)
  let problem = $state('')
  let sent = $state(false)
  let copied = $state(false)

  // titles of every note the list holds are taken out, as in the feedback form
  const titles = () => workspace.notes.map((n) => n.title)
  const error = errorCatch.caught ?? ''
  const about = `Error, ${scrub(error, titles())}`.slice(0, MAX_ABOUT)
  const details = scrub([error, errorCatch.where].filter(Boolean).join('\n'), titles())
  const logs = reportLogs(titles())
  const message = $derived(
    [doing.trim(), details].filter(Boolean).join('\n\n').slice(0, MAX_MESSAGE),
  )
  const contactBad = $derived(!contactOk(contact))

  onMount(() => {
    // a data link, the phone's webview leaves blob ones blank
    if (errorCatch.shot) {
      const reader = new FileReader()
      reader.onload = () => (shotUrl = String(reader.result))
      reader.readAsDataURL(errorCatch.shot)
    }
    backend
      .feedbackWhere()
      .then((where) => (place = where))
      .catch(() => (place = null))
  })

  function errorText(err: unknown): string {
    const e = err as { code?: string; retryAfterSeconds?: number } | null
    const code = e?.code ?? 'internal'
    const text = t(`feedback.error_${code}`, {
      minutes: Math.ceil((e?.retryAfterSeconds ?? 0) / 60),
    })
    return text === `feedback.error_${code}` ? t('feedback.error_internal') : text
  }

  async function send(event: SubmitEvent) {
    event.preventDefault()
    if (busy || contactBad) return
    busy = true
    problem = ''
    try {
      const image = withShot && errorCatch.shot ? await prepareImage(errorCatch.shot) : null
      const { secret, receipt } = await newReceipt()
      const done = await backend.feedbackSend({
        install: installId(),
        about,
        message,
        contact: contact.trim() || undefined,
        logs: logs || undefined,
        images: image ? [image.data] : [],
        receipt,
      })
      if (image) URL.revokeObjectURL(image.url)
      rememberSent({ secret, url: done.url, host: done.host, about, sentAt: Date.now() })
      sent = true
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(brand.contactEmail)
      copied = true
      setTimeout(() => (copied = false), 2000)
    } catch {
      // the address stays on screen to select by hand
    }
  }

  const close = () => !busy && errorCatch.dismiss()
</script>

<div
  class="backdrop"
  role="presentation"
  data-no-shot
  onclick={(e) => e.target === e.currentTarget && close()}
>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="error-report-title"
    data-error-report
    use:modal={{ onclose: close }}
  >
    <header>
      <h2 id="error-report-title">{t('errorReport.title')}</h2>
      <button
        class="icon-btn"
        type="button"
        aria-label={t('common.close')}
        disabled={busy}
        onclick={close}><Icon name="close" /></button
      >
    </header>

    {#if sent}
      <p role="status">{t('errorReport.sent')}</p>
      <div class="actions">
        <button class="btn primary" type="button" onclick={close}>{t('common.close')}</button>
      </div>
    {:else if place && !place.available}
      <p>{t('errorReport.off', { address: brand.contactEmail })}</p>
      <div class="actions">
        <button class="btn" type="button" onclick={copy}
          >{copied ? t('settings.contactCopied') : t('errorReport.copyAddress')}</button
        >
        <button class="btn primary" type="button" onclick={close}>{t('common.close')}</button>
      </div>
    {:else}
      <form onsubmit={send}>
        <p>{t('errorReport.lead')}</p>
        <label class="field-label">
          <span>{t('errorReport.doing')} <span class="muted">{t('common.optional')}</span></span>
          <textarea class="field" rows="3" maxlength={MAX_MESSAGE} bind:value={doing} data-autofocus
          ></textarea>
        </label>
        <label class="field-label">
          <span>{t('errorReport.contact')} <span class="muted">{t('common.optional')}</span></span>
          <input
            class="field"
            maxlength={MAX_CONTACT}
            autocomplete="email"
            aria-invalid={contactBad}
            bind:value={contact}
          />
          {#if contactBad}<span class="error">{t('feedback.contactBad')}</span>{/if}
        </label>
        {#if errorCatch.shot}
          <label class="shot">
            <input type="checkbox" bind:checked={withShot} data-error-shot />
            <span class="shot-text">
              {t('errorReport.screenshot')}
              <span class="muted">{t('errorReport.screenshotHelp')}</span>
            </span>
            {#if shotUrl}<img src={shotUrl} alt="" />{/if}
          </label>
        {/if}
        <details>
          <summary>{t('errorReport.whatIsSent')}</summary>
          <dl>
            <dt>{t('feedback.about')}</dt>
            <dd>{about}</dd>
            <dt>{t('feedback.info')}</dt>
            <dd><pre>{message}</pre></dd>
            {#if contact.trim()}
              <dt>{t('feedback.contact')}</dt>
              <dd>{contact.trim()}</dd>
            {/if}
            <dt>{t('feedback.app')}</dt>
            <dd>{place ? `${place.version}, ${place.platform}` : ''}</dd>
            <dt>{t('feedback.logs')}</dt>
            <dd>
              {#if logs}<pre>{logs}</pre>{:else}{t('feedback.noLogs')}{/if}
            </dd>
          </dl>
        </details>
        {#if place}<p class="muted">{t('errorReport.sentTo', { host: place.host })}</p>{/if}
        {#if problem}<p class="error" role="alert">{problem}</p>{/if}
        <div class="actions">
          <button class="btn" type="button" disabled={busy} onclick={close}
            >{t('common.cancel')}</button
          >
          <button class="btn primary" type="submit" disabled={busy || !place || contactBad}
            >{busy ? t('feedback.sending') : t('feedback.send')}</button
          >
        </div>
      </form>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    width: min(480px, 100%);
    max-height: calc(100vh - 2 * var(--space-4));
    overflow-y: auto;
    display: grid;
    gap: var(--space-3);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  form {
    display: grid;
    gap: var(--space-3);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 16px;
  }
  p {
    margin: 0;
  }
  .field-label {
    display: grid;
    gap: var(--space-1);
  }
  /* the fields' shared height is one line */
  textarea {
    height: auto;
    min-height: 4.5em;
    resize: vertical;
  }
  .muted {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .shot {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
  }
  .shot-text {
    display: grid;
  }
  .shot img {
    width: 72px;
    height: 48px;
    object-fit: cover;
    object-position: top;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control, 4px);
  }
  details {
    font-size: 13px;
  }
  summary {
    cursor: pointer;
    color: var(--color-accent-text);
  }
  dl {
    display: grid;
    gap: var(--space-1);
    margin: var(--space-2) 0 0;
  }
  dt {
    color: var(--color-text-muted);
  }
  dd {
    margin: 0 0 var(--space-1);
    overflow-wrap: anywhere;
  }
  pre {
    max-height: 160px;
    overflow: auto;
    margin: 0;
    padding: var(--space-2);
    background: var(--color-surface-2);
    border-radius: var(--radius-control, 4px);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .error {
    color: var(--color-danger);
    font-size: 13px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
