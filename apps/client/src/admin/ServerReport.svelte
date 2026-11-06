<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A report about this server itself, sent to the project's main server:
     this server forwards it, the page may only talk to its own server. -->
<script lang="ts">
  import { onDestroy } from 'svelte'
  import { MAX_IMAGES, prepareImage, type PreparedImage } from '../lib/feedback'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { api, ApiError } from './api.svelte'
  import { bytes, errorText } from './format'

  let about = $state('')
  let message = $state('')
  let contact = $state('')
  let busy = $state(false)
  let problem = $state('')
  let sent = $state(false)
  let images: PreparedImage[] = $state([])
  let dragging = $state(false)
  let picker: HTMLInputElement | undefined = $state()

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
    dragging = true
  }

  function drop(event: DragEvent) {
    event.preventDefault()
    dragging = false
    const files = imageFiles(event.dataTransfer?.files)
    if (files.length) void add(files)
    else problem = t('feedback.notAnImage')
  }

  async function send(event: SubmitEvent) {
    event.preventDefault()
    busy = true
    problem = ''
    try {
      await api('POST', '/admin/feedback/send', {
        about: about.trim(),
        message: message.trim(),
        contact: contact.trim() || undefined,
        images: images.map((i) => i.data),
      })
      sent = true
      about = message = contact = ''
      for (const image of images) URL.revokeObjectURL(image.url)
      images = []
    } catch (err) {
      // the main server's own words: this server only passed them on
      problem = err instanceof ApiError && err.code === 'internal' ? err.message : errorText(err)
    } finally {
      busy = false
    }
  }
</script>

<form class="send" onsubmit={send} onpaste={paste}>
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
  >
    <Icon name="image" size={16} />
    {images.length >= MAX_IMAGES
      ? t('feedback.tooManyImages', { count: MAX_IMAGES })
      : t('feedback.dropImages', { count: MAX_IMAGES })}
  </button>
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
  {#if images.length}
    <div class="images">
      {#each images as image, n (image.url)}
        <figure>
          <img src={image.url} alt={t('feedback.image', { n: n + 1 })} />
          <figcaption>
            {bytes(image.size)}
            <button class="btn" type="button" onclick={() => remove(n)}
              >{t('feedback.removeImage')}</button
            >
          </figcaption>
        </figure>
      {/each}
    </div>
  {/if}
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
  .drop {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    justify-self: stretch;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border: 1px dashed var(--color-border-strong);
    border-radius: var(--radius-popover);
    background: transparent;
    color: var(--color-text-muted);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  .drop:hover:not(:disabled),
  .drop.dragging {
    border-color: var(--color-accent);
    color: var(--color-text);
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
  .problem {
    color: var(--color-danger);
  }
  .saved {
    color: var(--color-accent-text);
  }
</style>
