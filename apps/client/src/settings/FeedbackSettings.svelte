<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Send feedback: the switch that offers it, where reports go, the form
     (what it is about, more words if needed, an email or a Discord name for
     an answer, recent errors unless unticked, images dropped, pasted or
     picked), a preview of exactly what goes, and Send. Nothing of the notes
     goes unless the person types or attaches it. Below, the reports sent
     from here and how they stand (SentFeedback). Settings groups, see
     app.css. -->
<script lang="ts">
  import { onDestroy, onMount } from 'svelte'
  import { backend, type FeedbackWhere } from '../lib/backend'
  import {
    contactOk,
    EMPTY_DRAFT,
    installId,
    loadDraft,
    MAX_ABOUT,
    MAX_CONTACT,
    MAX_IMAGES,
    MAX_MESSAGE,
    newReceipt,
    prepareImage,
    recentLogs,
    rememberSent,
    reportLogs,
    saveDraft,
    type PreparedImage,
  } from '../lib/feedback'
  import { bytes } from '../lib/format'
  import { t } from '../lib/i18n.svelte'
  import { workspace } from '../lib/workspace.svelte'
  import SentFeedback from './SentFeedback.svelte'

  let place: FeedbackWhere | null = $state(null)
  let placeProblem = $state('')
  let draft = $state(loadDraft())
  let images: PreparedImage[] = $state([])
  let sent = $state(false)
  let previewing = $state(false)
  /** The logs as the preview showed them, sent as they were seen. */
  let shownLogs = $state('')
  let dragging = $state(false)
  let busy = $state(false)
  let problem = $state('')
  let sentTo = $state('')
  /** Counts the reports sent while open, so the list below reads again. */
  let sentCount = $state(0)
  let picker: HTMLInputElement | undefined = $state()
  const errorCount = recentLogs().length
  // phones and tablets have nothing to drop from
  const touch = window.matchMedia('(pointer: coarse)').matches

  $effect(() => saveDraft($state.snapshot(draft)))

  const contactBad = $derived(!contactOk(draft.contact))
  const ready = $derived(draft.about.trim() !== '' && !contactBad)

  function errorText(err: unknown): string {
    const e = err as { code?: string; retryAfterSeconds?: number } | null
    const code = e?.code ?? 'internal'
    const text = t(`feedback.error_${code}`, {
      minutes: Math.ceil((e?.retryAfterSeconds ?? 0) / 60),
    })
    return text === `feedback.error_${code}` ? t('feedback.error_internal') : text
  }

  async function look() {
    placeProblem = ''
    try {
      place = await backend.feedbackWhere()
    } catch (err) {
      placeProblem = errorText(err)
    }
  }

  onMount(() => void look())
  onDestroy(() => {
    for (const image of images) URL.revokeObjectURL(image.url)
  })

  async function add(files: Iterable<Blob>) {
    problem = ''
    for (const file of files) {
      if (images.length >= MAX_IMAGES) {
        problem = t('feedback.tooManyImages', { count: MAX_IMAGES })
        return
      }
      const image = await prepareImage(file)
      if (image) images = [...images, image]
      else problem = t('feedback.notAnImage')
    }
  }

  function remove(n: number) {
    URL.revokeObjectURL(images[n]?.url ?? '')
    images = images.filter((_, i) => i !== n)
  }

  const imageFiles = (list: FileList | null | undefined) =>
    [...(list ?? [])].filter((f) => f.type.startsWith('image/'))

  function paste(event: ClipboardEvent) {
    const files = imageFiles(event.clipboardData?.files)
    if (files.length) {
      event.preventDefault()
      void add(files)
    }
  }

  function dragOver(event: DragEvent) {
    if (!event.dataTransfer?.types.includes('Files')) return
    event.preventDefault()
    event.dataTransfer.dropEffect = 'copy'
    dragging = true
  }

  function drop(event: DragEvent) {
    event.preventDefault()
    dragging = false
    const files = imageFiles(event.dataTransfer?.files)
    if (files.length) void add(files)
    else problem = t('feedback.notAnImage')
  }

  // titles of every note the list holds are taken out of the logs
  const logsNow = () => (draft.logs ? reportLogs(workspace.notes.map((n) => n.title)) : '')

  function togglePreview() {
    problem = ''
    if (!previewing) shownLogs = logsNow()
    previewing = !previewing
  }

  async function send() {
    busy = true
    problem = ''
    const logs = previewing ? shownLogs : logsNow()
    const about = draft.about.trim()
    try {
      const { secret, receipt } = await newReceipt()
      const done = await backend.feedbackSend({
        install: installId(),
        about,
        message: draft.message.trim(),
        contact: draft.contact.trim() || undefined,
        logs: logs || undefined,
        images: images.map((i) => i.data),
        receipt,
      })
      rememberSent({ secret, url: done.url, host: done.host, about, sentAt: Date.now() })
      sentCount++
      sentTo = done.host
      draft = { ...EMPTY_DRAFT }
      for (const image of images) URL.revokeObjectURL(image.url)
      images = []
      previewing = false
      sent = true
    } catch (err) {
      // the words stay as a draft to try again
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  const total = $derived(
    new TextEncoder().encode(draft.about + draft.message + draft.contact + shownLogs).length +
      images.reduce((sum, i) => sum + i.size, 0),
  )
</script>

{#if placeProblem}
  <p class="problem" role="alert">{placeProblem}</p>
  <button class="btn" onclick={() => void look()}>{t('feedback.retry')}</button>
{:else if place && !place.available}
  <p class="settings-help" data-feedback-off>{t(`feedback.unavailable_${place.via}`)}</p>
{:else if place}
  {#if sent}
    <div class="settings-group" data-feedback-sent>
      <p role="status">{t('feedback.sent', { host: sentTo })}</p>
      <button class="btn" onclick={() => (sent = false)}>{t('feedback.another')}</button>
    </div>
  {:else}
    <div
      class="settings-group wide"
      data-feedback-form
      onpaste={paste}
      role="group"
      aria-busy={busy}
    >
      <p class="settings-help">
        {place.via === 'server'
          ? t('feedback.toServer', { host: place.host })
          : t('feedback.toMain', { host: place.host })}
      </p>
      <label class="settings-field">
        {t('feedback.about')}
        <input
          class="field wide-field"
          maxlength={MAX_ABOUT}
          placeholder={t('feedback.aboutHint')}
          bind:value={draft.about}
          data-feedback-about
        />
      </label>
      <label class="settings-field">
        {t('feedback.info')}
        <textarea
          class="field wide-field message"
          rows="5"
          maxlength={MAX_MESSAGE}
          placeholder={t('feedback.infoHint')}
          bind:value={draft.message}
          data-feedback-info></textarea>
        {#if draft.message.length > MAX_MESSAGE * 0.8}
          <span class="count">{draft.message.length} / {MAX_MESSAGE}</span>
        {/if}
      </label>
      <label class="settings-field">
        {t('feedback.contact')}
        <input
          class="field wide-field"
          autocomplete="email"
          maxlength={MAX_CONTACT}
          aria-invalid={contactBad}
          bind:value={draft.contact}
          data-feedback-contact
        />
        <span class={contactBad ? 'problem small' : 'settings-help'}
          >{contactBad ? t('feedback.contactBad') : t('feedback.contactHelp')}</span
        >
      </label>
      <label class="settings-check">
        <input type="checkbox" bind:checked={draft.logs} data-feedback-logs />
        {errorCount
          ? t('feedback.attachLogsCount', { count: errorCount })
          : t('feedback.attachLogs')}
      </label>
      <button
        type="button"
        class="drop"
        class:dragging
        disabled={images.length >= MAX_IMAGES}
        onclick={() => picker?.click()}
        ondragenter={dragOver}
        ondragover={dragOver}
        ondragleave={() => (dragging = false)}
        ondrop={drop}
        data-feedback-drop
      >
        {images.length >= MAX_IMAGES
          ? t('feedback.tooManyImages', { count: MAX_IMAGES })
          : touch
            ? t('feedback.tapImages', { count: MAX_IMAGES })
            : t('feedback.dropImages', { count: MAX_IMAGES })}
      </button>
      {#if images.length}
        <div class="images">
          {#each images as image, n (image.url)}
            <figure>
              <img src={image.url} alt={t('feedback.image', { n: n + 1 })} />
              <figcaption>
                {bytes(image.size)}
                <button class="btn" onclick={() => remove(n)}>{t('feedback.removeImage')}</button>
              </figcaption>
            </figure>
          {/each}
        </div>
      {/if}
      <input
        bind:this={picker}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/*"
        multiple
        hidden
        onchange={(e) => {
          const input = e.currentTarget
          void add([...(input.files ?? [])]).then(() => (input.value = ''))
        }}
      />

      {#if previewing}
        <div class="preview" data-feedback-preview>
          <h3>{t('feedback.previewTitle', { host: place.host })}</h3>
          <dl>
            <dt>{t('feedback.about')}</dt>
            <dd class="text">{draft.about.trim() || t('feedback.none')}</dd>
            <dt>{t('feedback.info')}</dt>
            <dd class="text">{draft.message.trim() || t('feedback.none')}</dd>
            <dt>{t('feedback.contact')}</dt>
            <dd>{draft.contact.trim() || t('feedback.none')}</dd>
            <dt>{t('feedback.app')}</dt>
            <dd>{place.version} · {place.platform}</dd>
            <dt>{t('feedback.logs')}</dt>
            <dd>
              {#if shownLogs}<pre>{shownLogs}</pre>{:else if draft.logs}{t(
                  'feedback.noLogs',
                )}{:else}{t('feedback.none')}{/if}
            </dd>
            <dt>{t('feedback.images')}</dt>
            <dd>
              {images.length
                ? t('feedback.imageCount', { count: images.length })
                : t('feedback.none')}
            </dd>
            <dt>{t('feedback.size')}</dt>
            <dd>{bytes(total)}</dd>
          </dl>
          <p class="settings-help">{t('feedback.privacy')}</p>
        </div>
      {/if}

      {#if busy}<p role="status">{t('feedback.sending')}</p>{/if}
      <div class="settings-actions">
        <button class="btn" disabled={busy} onclick={togglePreview} data-feedback-preview-button
          >{previewing ? t('feedback.hidePreview') : t('feedback.preview')}</button
        >
        <button
          class="btn primary"
          disabled={busy || !ready}
          onclick={() => void send()}
          data-feedback-send>{t('feedback.send')}</button
        >
      </div>
    </div>
  {/if}
  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
{/if}
{#key sentCount}<SentFeedback />{/key}

<style>
  /* wider than the settings' usual fields, as wide as the drop zone */
  .settings-group .wide-field {
    width: min(560px, 100%);
  }
  .message {
    height: auto;
    resize: vertical;
  }
  .count {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .drop {
    width: min(560px, 100%);
    padding: var(--space-5) var(--space-4);
    border: 1px dashed var(--color-border);
    border-radius: var(--radius-popover);
    background: transparent;
    color: var(--color-text-muted);
    font: inherit;
    font-size: 13px;
    text-align: center;
    cursor: pointer;
  }
  .drop:hover:not(:disabled),
  .drop.dragging {
    border-color: var(--color-accent);
    color: var(--color-text);
  }
  .drop.dragging {
    background: var(--color-surface);
  }
  .drop:disabled {
    cursor: default;
  }
  .images {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  figure {
    display: grid;
    gap: var(--space-1);
    margin: 0;
  }
  figcaption {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 12px;
  }
  img {
    max-width: 160px;
    max-height: 120px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    object-fit: contain;
  }
  .preview {
    display: grid;
    gap: var(--space-2);
    width: min(560px, 100%);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
  }
  .preview h3 {
    margin: 0;
    font-size: 14px;
  }
  .preview p {
    margin: 0;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-1) var(--space-3);
    margin: 0;
  }
  dt {
    color: var(--color-text-muted);
  }
  dd {
    min-width: 0;
    margin: 0;
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  pre {
    max-height: 200px;
    overflow: auto;
    margin: 0;
    padding: var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .problem {
    color: var(--color-danger);
  }
  .small {
    font-size: 12px;
  }
  @media (max-width: 480px) {
    dl {
      grid-template-columns: 1fr;
    }
    dt {
      margin-top: var(--space-2);
    }
  }
</style>
