<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Temporary report button and dialog for dev and preview builds, until the
     real feedback feature replaces them. -->
<script lang="ts">
  import { tick } from 'svelte'
  import { SvelteMap } from 'svelte/reactivity'
  import { onBack } from '../lib/back'
  import { modal } from '../lib/modal'
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import { workspace } from '../lib/workspace.svelte'
  import {
    phoneFolder,
    phoneReports,
    pickPhoneFolder,
    recentErrors,
    saveReport,
    screenshot,
  } from './report'

  let open = $state(false)
  let text = $state('')
  let critical = $state(false)
  let images: File[] = $state([])
  let busy = $state(false)
  let saved = $state('')
  let failed = $state('')
  let fileInput: HTMLInputElement | undefined = $state()
  /** The screenshot taken as the window opened, first among the images. */
  let shot: File | null = null
  /** Hides the ! button while the phone takes its picture of the screen. */
  let capturing = $state(false)
  /** Phones save only into a folder the person picked (DevReports.kt). */
  const phone = phoneReports() !== undefined
  let folder = $state('')

  // data links rather than blob ones, which the phone's webview left blank
  const urls = new SvelteMap<File, string>()
  const previews = $derived(images.map((file) => urls.get(file) ?? ''))
  $effect(() => {
    for (const file of images) {
      if (urls.has(file)) continue
      const reader = new FileReader()
      reader.onload = () => urls.set(file, String(reader.result))
      reader.readAsDataURL(file)
    }
  })

  const frame = () => new Promise((done) => requestAnimationFrame(() => done(null)))

  $effect(() => {
    if (open) return onBack(close)
  })

  async function show() {
    if (capturing) return
    saved = failed = ''
    // read each time, the folder may have been picked in the settings since
    if (phone) folder = phoneFolder()
    if (prefs.reportScreenshot && (!phone || folder)) {
      // before the window opens, so it is not in the picture, and without
      // the ! button, which the phone's own picture would show
      capturing = true
      await tick()
      await frame()
      await frame()
      const fresh = await screenshot()
      capturing = false
      if (fresh) {
        images = [fresh, ...images.filter((f) => f !== shot)]
        shot = fresh
      }
    }
    open = true
  }

  async function chooseFolder() {
    folder = await pickPhoneFolder()
  }

  function close() {
    open = false
    if (saved) {
      text = ''
      critical = false
      images = []
      urls.clear()
      shot = null
      saved = ''
    }
  }

  function addImages(files: Iterable<File>) {
    const picked = [...files].filter((f) => f.type.startsWith('image/'))
    if (picked.length > 0) images = [...images, ...picked]
    return picked.length > 0
  }

  function onpaste(event: ClipboardEvent) {
    if (addImages(event.clipboardData?.files ?? [])) event.preventDefault()
  }

  function context(): string[] {
    const { innerWidth: w, innerHeight: h, devicePixelRatio: ratio } = window
    return [
      `Version: ${__APP_VERSION__}, built ${__APP_BUILD_DATE__}`,
      `Platform: ${navigator.userAgent}`,
      `Window: ${w}x${h} at ${ratio}x`,
      `Display: ${prefs.layout} layout, ${prefs.theme} theme, ${prefs.mode} mode, accent ${prefs.accent ?? 'theme'}, language ${prefs.language ?? 'system'}`,
      `Screen: ${workspace.current ? 'a note is open' : 'no note open'}, scope ${workspace.scope.kind}`,
    ]
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    busy = true
    failed = ''
    try {
      saved = await saveReport({
        text,
        critical,
        images,
        context: context(),
        errors: recentErrors(),
        now: new Date(),
      })
    } catch (err) {
      failed = String((err as { message?: string }).message ?? err)
    } finally {
      busy = false
    }
  }

  function onkeydown(event: KeyboardEvent) {
    const mod = event.ctrlKey || event.metaKey
    if (mod && event.shiftKey && event.key.toLowerCase() === 'b') {
      event.preventDefault()
      if (open) close()
      else void show()
    } else if (open && event.key === 'Escape') {
      close()
    }
  }
</script>

<svelte:window {onkeydown} />

<button
  class="launcher"
  class:capturing
  title={t('devReport.button')}
  aria-label={t('devReport.button')}
  data-no-shot
  onclick={() => void show()}>!</button
>

{#if open}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && close()}
  >
    <!-- on the dialog stack, so the settings under it stop pulling focus
         back; Esc is handled by onkeydown here -->
    <form class="dialog" aria-label={t('devReport.title')} onsubmit={submit} {onpaste} use:modal>
      <h2>{t('devReport.title')}</h2>
      {#if saved}
        <p>{t('devReport.saved')}</p>
        <code>{saved}</code>
        <div class="actions">
          <button class="btn primary" type="button" onclick={close}>{t('common.close')}</button>
        </div>
      {:else if phone && !folder}
        <p>{t('testTools.noFolder')}</p>
        <p class="muted">{t('testTools.folderHelpPhone')}</p>
        <div class="actions">
          <button class="btn" type="button" onclick={close}>{t('common.close')}</button>
          <button class="btn primary" type="button" onclick={chooseFolder}
            >{t('testTools.choose')}</button
          >
        </div>
      {:else}
        <p class="muted">{phone ? t('devReport.helpPhone') : t('devReport.help')}</p>
        <textarea
          class="field"
          rows="7"
          placeholder={t('devReport.placeholder')}
          aria-label={t('devReport.placeholder')}
          bind:value={text}
          data-autofocus></textarea>
        <div class="images">
          {#each images as file, i (file)}
            <figure>
              <img src={previews[i]} alt="" />
              <button
                type="button"
                class="icon-btn remove"
                aria-label={t('devReport.removeImage')}
                onclick={() => (images = images.filter((_, j) => j !== i))}>×</button
              >
            </figure>
          {/each}
        </div>
        <p class="muted">
          {t('devReport.pasteHint')}
          <button type="button" class="link" onclick={() => fileInput?.click()}
            >{t('devReport.addImage')}</button
          >
        </p>
        <input
          bind:this={fileInput}
          type="file"
          accept="image/*"
          multiple
          hidden
          onchange={(e) => {
            addImages(e.currentTarget.files ?? [])
            e.currentTarget.value = ''
          }}
        />
        <label class="critical">
          <input type="checkbox" bind:checked={critical} />
          <span>
            <strong>{t('devReport.critical')}</strong>
            <span class="muted">{t('devReport.criticalHelp')}</span>
          </span>
        </label>
        {#if failed}<p class="error" role="alert">{failed}</p>{/if}
        <div class="actions">
          <button class="btn" type="button" onclick={close}>{t('common.close')}</button>
          <button
            class="btn primary"
            type="submit"
            disabled={busy || (!text.trim() && images.length === 0)}
            >{busy ? t('devReport.saving') : t('devReport.save')}</button
          >
        </div>
      {/if}
    </form>
  </div>
{/if}

<style>
  .launcher {
    position: fixed;
    right: var(--space-3);
    bottom: var(--space-3);
    z-index: 40;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: 1px solid var(--color-border-strong);
    background: var(--color-surface-2);
    color: var(--color-text-muted);
    font-weight: 700;
    opacity: 0.6;
    cursor: pointer;
  }
  .launcher.capturing {
    visibility: hidden;
  }
  /* above the formatting bar, which sits at the bottom on touch screens */
  @media (pointer: coarse) {
    .launcher {
      bottom: 72px;
    }
  }
  .launcher:focus-visible {
    opacity: 1;
    color: var(--color-accent-text);
  }
  @media (hover: hover) {
    .launcher:hover {
      opacity: 1;
      color: var(--color-accent-text);
    }
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    width: min(520px, 100%);
    max-height: 100%;
    overflow: auto;
    display: grid;
    gap: var(--space-3);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  h2,
  p {
    margin: 0;
  }
  h2 {
    font-size: 18px;
  }
  textarea {
    resize: vertical;
    font: inherit;
  }
  code {
    overflow-wrap: anywhere;
    font-family: var(--font-mono);
    font-size: 13px;
  }
  .images {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .images:empty {
    display: none;
  }
  figure {
    position: relative;
    margin: 0;
  }
  figure img {
    display: block;
    height: 72px;
    max-width: 140px;
    object-fit: cover;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
  }
  .remove {
    position: absolute;
    top: 2px;
    right: 2px;
    background: var(--color-surface);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-accent-text);
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }
  .critical {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-2);
    align-items: start;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    cursor: pointer;
  }
  .critical:has(input:checked) {
    border-color: var(--color-danger);
  }
  .critical span {
    display: grid;
    gap: 2px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .error {
    color: var(--color-danger);
  }
</style>
