<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A report about this server itself, sent to the project's main server:
     this server forwards it, the page may only talk to its own server. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { api, ApiError } from './api.svelte'
  import { errorText } from './format'

  let about = $state('')
  let message = $state('')
  let contact = $state('')
  let busy = $state(false)
  let problem = $state('')
  let sent = $state(false)

  async function send(event: SubmitEvent) {
    event.preventDefault()
    busy = true
    problem = ''
    try {
      await api('POST', '/admin/feedback/send', {
        about: about.trim(),
        message: message.trim(),
        contact: contact.trim() || undefined,
      })
      sent = true
      about = message = contact = ''
    } catch (err) {
      // the main server's own words: this server only passed them on
      problem = err instanceof ApiError && err.code === 'internal' ? err.message : errorText(err)
    } finally {
      busy = false
    }
  }
</script>

<form class="send" onsubmit={send}>
  <p class="muted">{t('admin.feedback.sendHelp')}</p>
  {#if sent}<p class="saved" role="status">{t('admin.feedback.sendDone')}</p>{/if}
  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
  <label class="field-label">
    {t('admin.feedback.sendAbout')}
    <input class="field" bind:value={about} maxlength="120" required />
  </label>
  <label class="field-label">
    {t('admin.feedback.sendMessage')}
    <textarea class="field" rows="6" maxlength="5000" bind:value={message}></textarea>
  </label>
  <label class="field-label">
    {t('admin.feedback.sendContact')}
    <input class="field" bind:value={contact} maxlength="254" autocomplete="email" />
  </label>
  <button class="btn primary" type="submit" disabled={busy || !about.trim()}
    >{t('admin.feedback.sendButton')}</button
  >
</form>

<style>
  .send {
    display: grid;
    gap: var(--space-3);
    justify-items: start;
    max-width: 620px;
  }
  .send p {
    margin: 0;
  }
  .field-label {
    display: grid;
    gap: var(--space-1);
    justify-self: stretch;
  }
  textarea {
    min-height: 8em;
    resize: vertical;
  }
  .problem {
    color: var(--color-danger);
  }
  .saved {
    color: var(--color-accent-text);
  }
</style>
