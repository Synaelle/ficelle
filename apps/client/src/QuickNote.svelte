<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The desktop quick note window: a title, some text, Ctrl+Enter saves into
     the notes of the main window, Esc closes. Shares the display preferences
     of the main window but none of its screens. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { onMount } from 'svelte'
  import { backend } from './lib/backend'
  import { applyDyslexicFont } from './lib/dyslexicFont'
  import { setLocale, systemLocale, t } from './lib/i18n.svelte'
  import { prefs } from './lib/prefs.svelte'
  import { announceSaved } from './lib/quickNote'
  import { displayBinding } from './lib/shortcuts'
  import { applyTheme } from './lib/theme.svelte'
  import { applyUiSize } from './lib/uiSize'
  import { isBackendError } from './lib/workspace.svelte'

  applyTheme()
  $effect(() => setLocale(prefs.language ?? systemLocale()))
  $effect(() => applyUiSize(prefs.uiSize))
  $effect(() => applyDyslexicFont(prefs.dyslexicFont))

  let title = $state('')
  let body = $state('')
  let busy = $state(false)
  let problem = $state('')
  let bodyField: HTMLTextAreaElement | undefined = $state()

  const empty = $derived(!title.trim() && !body.trim())

  onMount(async () => {
    document.title = t('quick.title')
    const status = await backend.vaultStatus()
    if (!status.unlocked) problem = t('quick.locked', { app: brand.displayName })
    else bodyField?.focus()
  })

  async function save() {
    if (empty || busy) return
    busy = true
    try {
      await backend.createNote({ title: title.trim(), body: body.trim() })
      await announceSaved()
      await getCurrentWindow().close()
    } catch (err) {
      problem =
        isBackendError(err) && err.kind === 'locked'
          ? t('quick.locked', { app: brand.displayName })
          : t(`errors.${isBackendError(err) ? err.kind : 'unknown'}`)
    } finally {
      busy = false
    }
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') void getCurrentWindow().close()
    else if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault()
      void save()
    }
  }
</script>

<svelte:window {onkeydown} />

<main>
  <h1 class="visually-hidden">{t('quick.title')}</h1>
  <input
    class="title"
    placeholder={t('quick.titlePlaceholder')}
    aria-label={t('note.title')}
    bind:value={title}
  />
  <textarea
    placeholder={t('quick.bodyPlaceholder')}
    aria-label={t('quick.title')}
    bind:value={body}
    bind:this={bodyField}></textarea>
  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
  <footer>
    <span class="muted">{t('quick.hint', { keys: displayBinding('Mod+Enter') })}</span>
    <button class="btn primary" disabled={empty || busy} onclick={save}>{t('quick.save')}</button>
  </footer>
</main>

<style>
  main {
    height: 100%;
    display: grid;
    grid-template-rows: auto 1fr auto auto;
    gap: var(--space-2);
    padding: var(--space-3);
  }
  .title {
    border: 0;
    background: none;
    font-size: 18px;
    font-weight: 600;
    outline: none;
  }
  textarea {
    resize: none;
    border: 0;
    background: none;
    font-family: var(--font-editor);
    line-height: 1.5;
    outline: none;
  }
  .problem {
    margin: 0;
    color: var(--color-warning);
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  footer .muted {
    font-size: 12px;
  }
</style>
