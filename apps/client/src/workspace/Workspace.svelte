<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Main screen. Standard: sidebar, list, note. Lite: list and note only.
     Narrow windows and phones show the list or the note, never both. -->
<script lang="ts">
  import ShareDialog from './ShareDialog.svelte'
  import { sharing } from '../lib/sharing.svelte'
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
  import { instance } from '../lib/instance.svelte'
  import { backups } from '../lib/backups.svelte'
  import { onMount } from 'svelte'
  import Settings from '../settings/Settings.svelte'
  import AttachmentList from './AttachmentList.svelte'
  import NoteList from './NoteList.svelte'
  import NoteView from './NoteView.svelte'
  import Sidebar from './Sidebar.svelte'
  import Splitter from './Splitter.svelte'

  /** Only an encrypted vault has anything to lock. */
  let { onlock, lockable }: { onlock: () => void; lockable: boolean } = $props()

  let width = $state(window.innerWidth)
  let sidebarOpen = $state(true)
  let settingsOpen = $state(false)
  let settingsSection: 'sync' | undefined = $state()
  function openSettings(section?: 'sync') {
    settingsSection = section
    settingsOpen = true
  }
  let searchFocus = $state(0)

  const NARROW = 720
  const narrow = $derived(width < NARROW)
  const showSidebar = $derived(prefs.layout === 'standard' && sidebarOpen && !narrow)
  // the note keeps at least this much room when the list grows
  const NOTE_MIN = 360
  const listMax = $derived(Math.max(240, width - (showSidebar ? prefs.sidebarWidth : 0) - NOTE_MIN))

  void workspace.load()
  onMount(() => {
    // what the instance offers (its themes), heard again
    void instance.refresh(true)
    // scheduled backups run while the notes are open
    backups.start()
    return () => backups.stop()
  })
  void sync.start()

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
    toggleSidebar: () => (sidebarOpen = !sidebarOpen),
    toggleLayout: () => (prefs.layout = prefs.layout === 'lite' ? 'standard' : 'lite'),
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
      else if (item.kind === 'openNote') await workspace.open(item.id).catch(() => {})
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

  // save before the window goes away
  $effect(() => {
    const save = () => void workspace.flush()
    window.addEventListener('beforeunload', save)
    return () => window.removeEventListener('beforeunload', save)
  })
</script>

<svelte:window bind:innerWidth={width} {onkeydown} />

<main class="workspace" class:narrow class:lite={prefs.layout === 'lite'}>
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

{#if sharing.target}
  <ShareDialog target={sharing.target} />
{/if}

{#if settingsOpen}
  <Settings close={() => (settingsOpen = false)} {onlock} start={settingsSection} />
{/if}

{#if workspace.error}
  <div class="toast" role="alert">
    {t(`errors.${workspace.error}`)}
    <button class="icon-btn" aria-label={t('common.close')} onclick={() => (workspace.error = null)}
      >×</button
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
