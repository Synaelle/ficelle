<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Main screen: sidebar, list, note.
     Narrow windows and phones show the list or the note, never both. -->
<script lang="ts">
  import { localSync } from '../lib/localSync.svelte'
  import { sync } from '../lib/sync.svelte'
  import { onBack } from '../lib/back'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import { isMarkdownFile } from '../lib/importing'
  import { saveShare, takeInbox } from '../lib/inbox'
  import { updateWidget } from '../lib/widget'
  import { onQuickNoteSaved, quickNoteSupported } from '../lib/quickNote'
  import { prefs } from '../lib/prefs.svelte'
  import { bindingOf, type ActionId } from '../lib/shortcuts'
  import { shortcutFor } from '../lib/prefs.svelte'
  import { workspace } from '../lib/workspace.svelte'
  import { backups } from '../lib/backups.svelte'
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { DESKTOP, MOBILE } from '../lib/platform'
  import { sendSyncedPrefs, takeSyncedPrefs } from '../lib/syncedPrefs.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import Settings from '../settings/Settings.svelte'
  import AttachmentList from './AttachmentList.svelte'
  import NoteList from './NoteList.svelte'
  import NoteView from './NoteView.svelte'
  import Sidebar from './Sidebar.svelte'
  import Splitter from './Splitter.svelte'

  /** Only an encrypted vault has anything to lock. */
  let { onlock, lockable }: { onlock: () => void; lockable: boolean } = $props()

  let width = $state(window.innerWidth)
  let settingsOpen = $state(false)
  let settingsSection: 'sync' | undefined = $state()
  function openSettings(section?: 'sync') {
    settingsSection = section
    settingsOpen = true
  }
  let searchFocus = $state(0)

  const NARROW = 720
  /** A phone, not a tablet: turned sideways it is wide enough for three
   * panes, but with the keyboard up the note would have no room left. */
  const PHONE = MOBILE && Math.min(screen.width, screen.height) < 600
  const narrow = $derived(width < NARROW || PHONE)
  const showSidebar = $derived(!narrow)
  // the note keeps at least this much room when the list grows
  const NOTE_MIN = 360
  const listMax = $derived(Math.max(240, width - (showSidebar ? prefs.sidebarWidth : 0) - NOTE_MIN))

  void workspace.load()
  onMount(() => {
    // scheduled backups run while the notes are open
    backups.start()
    return () => backups.stop()
  })
  void sync.start()
  void localSync.start()
  void takeSyncedPrefs()
  sendSyncedPrefs()

  // On phones back walks out the way the user came in: settings, the note,
  // then the search. Booleans, so switching notes keeps the order.
  const searching = $derived(narrow && workspace.query.trim() !== '')
  const noteShown = $derived(narrow && workspace.current !== null)
  $effect(() => {
    if (searching) return onBack(() => void workspace.search(''))
  })
  $effect(() => {
    if (noteShown) return onBack(() => void workspace.open(null))
  })
  $effect(() => {
    if (settingsOpen) return onBack(() => (settingsOpen = false))
  })

  const actions: Partial<Record<ActionId, () => void>> = {
    newNote: () => void workspace.create(),
    search: () => (searchFocus += 1),
    pin: () =>
      workspace.current &&
      void workspace.togglePin(workspace.current.id, !workspace.current.pinned),
    trash: () => workspace.current && void workspace.trash(workspace.current.id),
    settings: () => (settingsOpen = true),
  }

  function onkeydown(event: KeyboardEvent) {
    const binding = bindingOf(event)
    if (!binding) return
    for (const [action, run] of Object.entries(actions)) {
      if (shortcutFor(action as ActionId) === binding) {
        event.preventDefault()
        run()
        return
      }
    }
  }

  const hasFiles = (event: DragEvent) => event.dataTransfer?.types.includes('Files') ?? false

  // Capture phase so markdown files become notes even when dropped on the
  // editor, which would otherwise take them as attachments.
  function importDrop(event: DragEvent) {
    const files = [...(event.dataTransfer?.files ?? [])]
    if (!files.some(isMarkdownFile)) return
    event.preventDefault()
    event.stopPropagation()
    void workspace.importFiles(files)
  }

  // A file dropped where nothing takes it would replace the whole page.
  function ignoreDrop(event: DragEvent) {
    if (hasFiles(event)) event.preventDefault()
  }

  $effect(() => {
    window.addEventListener('drop', importDrop, true)
    window.addEventListener('dragover', ignoreDrop)
    window.addEventListener('drop', ignoreDrop)
    return () => {
      window.removeEventListener('drop', importDrop, true)
      window.removeEventListener('dragover', ignoreDrop)
      window.removeEventListener('drop', ignoreDrop)
    }
  })

  // Shares and quick actions that came in while the app was closed or locked
  // run now, later ones when MainActivity says they arrived.
  async function runInbox() {
    for (const item of takeInbox()) {
      settingsOpen = false
      if (item.kind === 'newNote') await workspace.create()
      else if (item.kind === 'share') await saveShare(item)
      else if (item.kind === 'openNote')
        await workspace.open(item.id).catch((err: unknown) => workspace.report(err))
      else {
        if (narrow) await workspace.open(null)
        searchFocus += 1
      }
    }
  }
  $effect(() => {
    void runInbox()
    window.__appInbox = () => void runInbox()
    return () => delete window.__appInbox
  })

  // the home screen widget follows the list and the language, a short wait
  // keeps a burst of saves to one update
  $effect(() => {
    void workspace.notes
    void currentLocale()
    const timer = setTimeout(() => void updateWidget(), 400)
    return () => clearTimeout(timer)
  })

  // a note saved from the desktop quick note window shows up in the list
  $effect(() => {
    if (!quickNoteSupported) return
    const stop = onQuickNoteSaved(() => void workspace.refreshNotes())
    return () => void stop.then((unlisten) => unlisten())
  })

  // save before the window goes away: beforeunload cannot wait for the save,
  // so a desktop window holds its close until the save is done, and a phone
  // saves as the app leaves the screen, where it may be stopped unasked
  $effect(() => {
    const save = () => void workspace.flush()
    const hidden = () => document.visibilityState === 'hidden' && save()
    window.addEventListener('beforeunload', save)
    document.addEventListener('visibilitychange', hidden)
    const unlisten = DESKTOP ? getCurrentWindow().onCloseRequested(() => workspace.flush()) : null
    return () => {
      window.removeEventListener('beforeunload', save)
      document.removeEventListener('visibilitychange', hidden)
      void unlisten?.then((stop) => stop())
    }
  })
</script>

<svelte:window bind:innerWidth={width} {onkeydown} />

<main class="workspace" class:narrow>
  {#if showSidebar}
    <Sidebar onsettings={openSettings} {onlock} {lockable} />
    <Splitter
      bind:width={prefs.sidebarWidth}
      min={170}
      max={400}
      initial={220}
      label={t('workspace.resizeSidebar')}
    />
  {/if}
  {#if !narrow || !workspace.current}
    {#if workspace.files}
      <AttachmentList />
    {:else}
      <NoteList {narrow} {searchFocus} onsettings={openSettings} />
    {/if}
  {/if}
  {#if !narrow}
    <Splitter
      bind:width={prefs.listWidth}
      min={240}
      max={listMax}
      initial={320}
      label={t('workspace.resizeList')}
    />
  {/if}
  {#if !narrow || workspace.current}
    <NoteView {narrow} />
  {/if}
</main>

{#if settingsOpen}
  <Settings close={() => (settingsOpen = false)} {onlock} start={settingsSection} />
{/if}

{#if workspace.error}
  <div class="toast" role="alert">
    {t(`errors.${workspace.error}`)}
    <button class="icon-btn" aria-label={t('common.close')} onclick={() => (workspace.error = null)}
      ><Icon name="close" size={16} /></button
    >
  </div>
{/if}

<style>
  .workspace {
    height: 100%;
    display: flex;
    min-width: 0;
  }
  .toast {
    position: fixed;
    left: 50%;
    bottom: var(--space-5);
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-4);
    background: var(--color-surface-2);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
    color: var(--color-text);
  }
</style>
