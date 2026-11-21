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
  import Editor from '../editor/Editor.svelte'
  import { autoLock } from '../lib/autolock.svelte'
  import { backend, type VaultStatus } from '../lib/backend'
  import { applyDyslexicFont } from '../lib/dyslexicFont'
  import Icon from '../lib/icons/Icon.svelte'
  import { setLocale, systemLocale, t } from '../lib/i18n.svelte'
  import { parseMarkdownFile, parseTextFile } from '../lib/importing'
  import { onBack } from '../lib/back'
  import { modal } from '../lib/modal'
  import { overlay } from '../lib/overlay.svelte'
  import Overlays from '../lib/Overlays.svelte'
  import { DESKTOP } from '../lib/platform'
  import { prefs } from '../lib/prefs.svelte'
  import { applyTheme } from '../lib/theme.svelte'
  import { toasts } from '../lib/toast.svelte'
  import Toasts from '../lib/Toasts.svelte'
  import { applyUiSize } from '../lib/uiSize'
  import { announceUnlocked, windowReady } from '../lib/windows'
  import {
    mainWindowOpen,
    readOpenedFile,
    showInMain,
    writeOpenedFile,
    type OpenedFile,
  } from './files'
  import { isBackendError } from '../lib/workspace.svelte'
  import Unlock from '../screens/Unlock.svelte'

  let { close }: { close: () => void } = $props()

  // a window of its own on desktops; on the phone the app around it does this
  if (DESKTOP) {
    applyTheme(() => backend.systemClock24h())
    $effect(() => void setLocale(prefs.language ?? systemLocale()))
    $effect(() => applyUiSize(prefs.uiSize))
    $effect(() => applyDyslexicFont(prefs.dyslexicFont))
    // notes opened here for an import, with no main window whose auto-lock
    // would close them again: this window's does
    autoLock(() => {
      if (unlockedHere)
        void mainWindowOpen().then((main) => (main ? undefined : backend.vaultLock()))
    })
  }

  /** Wait after the last keystroke before writing the file, as notes do. */
  const SAVE_DELAY_MS = 1000

  type Problem = 'changedOnDisk' | 'fileGone' | 'notSaved' | 'readOnly'

  let file = $state<OpenedFile | null>(null)
  let failed = $state<'notText' | 'failed' | null>(null)
  let name = $state('')
  /** The text as the editor has it, written after a pause. */
  let text = ''
  /** The editor's Markdown: what it opened with, then what it passed on,
   * so it keeps the file's own form of the lines left alone. */
  let shown = $state('')
  /** Kept out of the editor and written back around its text: the mark some
   * Windows apps start a file with, and a Markdown file's front matter,
   * which the editor would read as a heading. */
  let bom = ''
  let front = $state('')
  /** A file with Windows line endings: the text box drops them, and the
   * editor reads a line break before one as a hard break. */
  let crlf = false
  let dirty = $state(false)
  let problem = $state<Problem | null>(null)
  /** Grows when the file is read again, the editor then starts over. */
  let revision = $state(0)
  let timer: ReturnType<typeof setTimeout> | undefined
  let writing: Promise<void> | null = null
  let editor: Editor | undefined = $state()
  /** The note the file became, once imported. */
  let imported = $state<string | null>(null)
  let unlocking = $state<VaultStatus | null>(null)
  /** This window opened the notes for an import. */
  let unlockedHere = false
  let importing = $state(false)

  function show(read: OpenedFile) {
    let content = read.text
    bom = content.startsWith('\uFEFF') ? '\uFEFF' : ''
    content = content.slice(bom.length)
    crlf = content.includes('\r\n')
    if (crlf) content = content.replaceAll('\r\n', '\n')
    front = read.plain ? '' : (/^---\r?\n[\s\S]*?\r?\n---[ \t]*(\r?\n|$)/.exec(content)?.[0] ?? '')
    content = content.slice(front.length)
    file = read
    name = read.name
    text = content
    shown = content
    dirty = false
    problem = read.readOnly ? 'readOnly' : null
    revision += 1
    document.title = read.name
  }

  async function load() {
    try {
      show(await readOpenedFile())
    } catch (err) {
      // read again once another app moved or deleted it: what shows stays
      if (file) problem = 'fileGone'
      else {
        failed = isBackendError(err) && err.kind === 'notText' ? 'notText' : 'failed'
        if (DESKTOP) name = await getCurrentWindow().title()
      }
    }
  }

  /** The file as written: the text with what was kept out of the editor. */
  const whole = (body: string) =>
    bom + (crlf ? (front + body).replaceAll('\n', '\r\n') : front + body)

  function edited(next: string) {
    if (!file || next === text) return
    text = next
    if (!file.plain) shown = next
    dirty = true
    clearTimeout(timer)
    timer = setTimeout(() => void save(), SAVE_DELAY_MS)
  }

  /** Writes the text now, after a write on its way. With `over`, writes
   * over a change another app made, or again where the file was. */
  async function save(over = false) {
    // the editor passes typing on after a short pause: taken now
    editor?.flush()
    clearTimeout(timer)
    while (writing) await writing
    if (!file || !dirty || (problem && !over && problem !== 'notSaved')) return
    const sent = text
    writing = writeOpenedFile(whole(sent), over ? null : file.stamp)
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
          kind === 'changedOnDisk' || kind === 'fileGone' || kind === 'readOnly'
            ? kind
            : file?.stamp === null && refused
              ? 'readOnly'
              : 'notSaved'
      })
      .finally(() => (writing = null))
    await writing
  }

  async function reload() {
    if (
      dirty &&
      !(await overlay.ask(t('file.reloadSure'), { confirmLabel: t('file.reload'), danger: true }))
    )
      return
    await load()
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
        : parseMarkdownFile(file.name, front + text)
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
    unlockedHere = true
    void announceUnlocked()
    await importFile()
  }

  /** Closes once the text is written, or when the person lets it go. */
  async function leave() {
    await save()
    if (
      !dirty ||
      (await overlay.ask(t('file.closeUnsaved'), {
        confirmLabel: t('file.closeAnyway'),
        danger: true,
      }))
    )
      close()
  }

  // another app changed the file while nothing was typed here: shown at once
  async function lookAgain() {
    if (!file || dirty || problem || file.stamp === null) return
    const read = await readOpenedFile().catch(() => null)
    if (read && file && read.stamp !== file.stamp && !dirty) show(read)
  }

  // the window's own close writes what is waiting first
  const stopClose = DESKTOP
    ? getCurrentWindow().onCloseRequested(async (event) => {
        event.preventDefault()
        await leave()
      })
    : null
  if (DESKTOP) window.addEventListener('focus', lookAgain)
  onDestroy(() => {
    clearTimeout(timer)
    void stopClose?.then((stop) => stop())
    window.removeEventListener('focus', lookAgain)
  })

  // the phone's back leaves the file, after writing it, or the unlock first
  $effect(() => {
    if (!DESKTOP) return onBack(() => void leave())
  })
  $effect(() => {
    if (unlocking) return onBack(() => (unlocking = null))
  })

  void load().finally(() => {
    if (DESKTOP) void windowReady()
  })
</script>

<main class="file" aria-label={name}>
  <header>
    {#if !DESKTOP}
      <button class="icon-btn" aria-label={t('common.close')} onclick={() => void leave()}>
        <Icon name="back" />
      </button>
    {/if}
    <h1 title={name}>{name}</h1>
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
    {:else if failed && DESKTOP}
      <button class="btn" onclick={close}>{t('common.close')}</button>
    {/if}
  </header>

  {#if problem === 'changedOnDisk'}
    <p class="problem" role="alert">
      <span>{t('file.changedOnDisk')}</span>
      <button class="btn" onclick={() => void reload()}>{t('file.reload')}</button>
      <button class="btn" onclick={() => void save(true)}>{t('file.keepMine')}</button>
    </p>
  {:else if problem === 'fileGone'}
    <p class="problem" role="alert">
      <span>{t('file.fileGone')}</span>
      <button class="btn" onclick={() => void save(true)}>{t('file.saveAgain')}</button>
    </p>
  {:else if problem}
    <p class="problem" role="alert">{t(`file.${problem}`)}</p>
  {/if}

  {#if failed}
    <p class="empty" role="alert">{t(`file.${failed}`)}</p>
  {:else if file}
    {#key revision}
      <div class="scroll">
        {#if file.plain}
          <textarea
            class="plain"
            aria-label={file.name}
            spellcheck={prefs.spellcheck}
            value={shown}
            oninput={(e) => edited(e.currentTarget.value)}
            onblur={() => void save()}></textarea>
        {:else}
          <article>
            {#if front}
              <details class="front">
                <summary>{t('file.frontMatter')}</summary>
                <pre>{front.trim()}</pre>
              </details>
            {/if}
            <Editor
              bind:this={editor}
              noteId=""
              markdown={shown}
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
    <p class="unlock-for">
      <span>{t('file.unlockToImport', { name })}</span>
      <button class="btn" onclick={() => (unlocking = null)}>{t('common.cancel')}</button>
    </p>
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
  .front {
    margin-bottom: var(--space-4);
    font-size: 13px;
    color: var(--color-text-muted);
  }
  .front summary {
    cursor: pointer;
  }
  .front pre {
    margin: var(--space-2) 0 0;
    font-family: var(--font-mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .unlock-for {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }
  .unlock-for span {
    flex: 1;
    min-width: 12ch;
    overflow-wrap: anywhere;
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
