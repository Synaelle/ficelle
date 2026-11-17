<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A Markdown or text file opened with the app: the editor for that one
     file, without the notes. It reads and writes the file itself and needs
     no unlock. Nothing of the file reaches the notes, and nothing of the
     notes shows here, until the person imports it, which asks for the
     unlock when the notes are locked. A desktop window of its own; on the
     phone a screen over the app, which `close` leaves. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { onDestroy } from 'svelte'
  import Editor from './editor/Editor.svelte'
  import { backend, type VaultStatus } from './lib/backend'
  import { applyDyslexicFont } from './lib/dyslexicFont'
  import Icon from './lib/icons/Icon.svelte'
  import { setLocale, systemLocale, t } from './lib/i18n.svelte'
  import { parseMarkdownFile, parseTextFile } from './lib/importing'
  import { onBack } from './lib/back'
  import { modal } from './lib/modal'
  import Overlays from './lib/Overlays.svelte'
  import { DESKTOP } from './lib/platform'
  import { prefs } from './lib/prefs.svelte'
  import { applyTheme } from './lib/theme.svelte'
  import { toasts } from './lib/toast.svelte'
  import Toasts from './lib/Toasts.svelte'
  import { applyUiSize } from './lib/uiSize'
  import {
    announceUnlocked,
    readOpenedFile,
    showInMain,
    windowReady,
    writeOpenedFile,
    type OpenedFile,
  } from './lib/windows'
  import { isBackendError } from './lib/workspace.svelte'
  import Unlock from './screens/Unlock.svelte'

  let { close }: { close: () => void } = $props()

  // a window of its own on desktops; on the phone the app around it does this
  if (DESKTOP) {
    applyTheme(() => backend.systemClock24h())
    $effect(() => void setLocale(prefs.language ?? systemLocale()))
    $effect(() => applyUiSize(prefs.uiSize))
    $effect(() => applyDyslexicFont(prefs.dyslexicFont))
  }

  /** Wait after the last keystroke before writing the file, as notes do. */
  const SAVE_DELAY_MS = 1000

  let file = $state<OpenedFile | null>(null)
  let failed = $state(false)
  /** The text as the editor has it, written after a pause. */
  let text = ''
  let dirty = $state(false)
  let problem = $state<'changedOnDisk' | 'notSaved' | 'readOnly' | null>(null)
  /** Grows when the file is read again, the editor then starts over. */
  let revision = $state(0)
  let timer: ReturnType<typeof setTimeout> | undefined
  let writing: Promise<void> | null = null
  /** The note the file became, once imported. */
  let imported = $state<string | null>(null)
  let unlocking = $state<VaultStatus | null>(null)
  let importing = $state(false)

  async function load() {
    try {
      file = await readOpenedFile()
      text = file.text
      dirty = false
      problem = null
      revision += 1
      document.title = file.name
    } catch {
      failed = true
    }
  }

  function edited(next: string) {
    if (!file || next === text) return
    text = next
    dirty = true
    clearTimeout(timer)
    timer = setTimeout(() => void save(), SAVE_DELAY_MS)
  }

  /** Writes the text now, after a write on its way. With `over`, writes
   * over a change another app made. */
  async function save(over = false) {
    clearTimeout(timer)
    while (writing) await writing
    if (!file || !dirty || (problem && !over && problem !== 'notSaved')) return
    const sent = text
    writing = writeOpenedFile(sent, over ? null : file.stamp)
      .then((stamp) => {
        if (file) file.stamp = stamp
        dirty = text !== sent
        problem = null
      })
      .catch((err: unknown) => {
        const kind = isBackendError(err) ? err.kind : 'unknown'
        // an app that let Ficelle read its file without writing it (Android)
        const refused = isBackendError(err) && /denied|permission/i.test(err.message)
        problem =
          kind === 'changedOnDisk'
            ? 'changedOnDisk'
            : file?.stamp === null && refused
              ? 'readOnly'
              : 'notSaved'
      })
      .finally(() => (writing = null))
    await writing
  }

  async function importFile() {
    if (!file || importing) return
    importing = true
    try {
      const status = await backend.vaultStatus()
      if (!status.unlocked) {
        unlocking = status
        return
      }
      const parsed = file.plain
        ? parseTextFile(file.name, text)
        : parseMarkdownFile(file.name, text)
      const note = await backend.createNote(parsed)
      imported = note.id
      toasts.push(file.name, t('file.imported', { title: note.title || t('note.untitled') }))
    } catch (err) {
      toasts.push(file.name, t(`errors.${isBackendError(err) ? err.kind : 'unknown'}`))
    } finally {
      importing = false
    }
  }

  async function unlocked() {
    unlocking = null
    void announceUnlocked()
    await importFile()
  }

  async function leave() {
    await save()
    if (!dirty || problem === 'readOnly') close()
  }

  // the window's own close writes what is waiting first; a file that cannot
  // be written keeps its window, with the reason shown
  const stopClose = DESKTOP
    ? getCurrentWindow().onCloseRequested(async (event) => {
        event.preventDefault()
        await save()
        if (!dirty || problem === 'readOnly') close()
      })
    : null
  onDestroy(() => {
    clearTimeout(timer)
    void stopClose?.then((stop) => stop())
  })

  // the phone's back leaves the file, after writing it
  $effect(() => {
    if (!DESKTOP) return onBack(() => void leave())
  })

  void load().finally(() => {
    if (DESKTOP) void windowReady()
  })
</script>

<main class="file" aria-label={file?.name ?? ''}>
  <header>
    {#if !DESKTOP}
      <button class="icon-btn" aria-label={t('common.close')} onclick={() => void leave()}>
        <Icon name="back" />
      </button>
    {/if}
    <h1 title={file?.name}>{file?.name ?? ''}</h1>
    {#if file && !problem}
      <span class="saved muted" class:quiet={!dirty}>
        <span class="mark" class:done={!dirty} aria-hidden="true">
          <Icon name="check" size={12} />
        </span>
      </span>
    {/if}
    {#if imported}
      <button class="btn" onclick={() => imported && void showInMain(imported).then(close)}>
        {t('file.openInApp', { app: brand.displayName })}
      </button>
    {:else if file}
      <button class="btn primary" disabled={importing} onclick={() => void importFile()}>
        {t('file.import', { app: brand.displayName })}
      </button>
    {/if}
  </header>

  {#if problem === 'changedOnDisk'}
    <p class="problem" role="alert">
      <span>{t('file.changedOnDisk')}</span>
      <button class="btn" onclick={() => void load()}>{t('file.reload')}</button>
      <button class="btn" onclick={() => void save(true)}>{t('file.keepMine')}</button>
    </p>
  {:else if problem}
    <p class="problem" role="alert">{t(`file.${problem}`)}</p>
  {/if}

  {#if failed}
    <p class="empty" role="alert">{t('file.failed')}</p>
  {:else if file}
    {#key revision}
      <div class="scroll">
        {#if file.plain}
          <textarea
            class="plain"
            aria-label={file.name}
            spellcheck={prefs.spellcheck}
            value={file.text}
            oninput={(e) => edited(e.currentTarget.value)}
            onblur={() => void save()}></textarea>
        {:else}
          <article>
            <Editor
              noteId=""
              markdown={file.text}
              onchange={edited}
              onopenlink={async () => false}
              loose
            />
          </article>
        {/if}
      </div>
    {/key}
  {/if}
</main>

{#if unlocking}
  <div
    class="unlocking"
    role="dialog"
    aria-modal="true"
    aria-label={t('unlock.title')}
    use:modal={{ onclose: () => (unlocking = null) }}
  >
    <Unlock done={unlocked} status={unlocking} />
  </div>
{/if}

{#if DESKTOP}
  <Overlays />
  <Toasts />
{/if}

<style>
  .file {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--color-surface);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }
  h1 {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .saved {
    display: inline-flex;
    transition: opacity 400ms;
  }
  .saved.quiet {
    opacity: 0;
  }
  .mark:not(.done) {
    opacity: 0.4;
  }
  .problem {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    padding: var(--space-2) var(--space-3);
    color: var(--color-warning);
    border-bottom: 1px solid var(--color-border);
  }
  .problem span {
    flex: 1;
    min-width: 12ch;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  article {
    min-height: 100%;
    padding: var(--space-6) var(--space-6) 120px;
  }
  @media (max-width: 600px) {
    article {
      padding: var(--space-4) var(--space-4) 120px;
    }
  }
  .plain {
    display: block;
    width: 100%;
    height: 100%;
    padding: var(--space-4);
    border: 0;
    resize: none;
    background: none;
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: 14px;
    line-height: 1.55;
    tab-size: 4;
    outline: none;
  }
  .plain:focus-visible {
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }
  .empty {
    margin: auto;
    padding: var(--space-4);
    color: var(--color-text-muted);
  }
  .unlocking {
    position: fixed;
    inset: 0;
    z-index: 80;
    overflow-y: auto;
    background: var(--color-bg);
  }
  @media (prefers-reduced-motion: reduce) {
    .saved {
      transition: none;
    }
  }
</style>
