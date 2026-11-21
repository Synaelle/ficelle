<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A note in a window of its own (desktop): the open note as in the main
     window, without the tags and the list. It locks and unlocks with the
     app, and closes when its note goes to the trash or away. -->
<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { onDestroy } from 'svelte'
  import { backend, type VaultStatus } from './lib/backend'
  import { applyDyslexicFont } from './lib/dyslexicFont'
  import { setLocale, systemLocale, t } from './lib/i18n.svelte'
  import Overlays from './lib/Overlays.svelte'
  import { localSync } from './lib/localSync.svelte'
  import { prefs } from './lib/prefs.svelte'
  import { sync } from './lib/sync.svelte'
  import { applyTheme } from './lib/theme.svelte'
  import Toasts from './lib/Toasts.svelte'
  import { applyUiSize } from './lib/uiSize'
  import {
    announceUnlocked,
    onChangedElsewhere,
    onFlushAsked,
    onLocked,
    onUnlockedElsewhere,
    reportActivity,
    windowReady,
  } from './lib/windows'
  import { workspace } from './lib/workspace.svelte'
  import Unlock from './screens/Unlock.svelte'
  import NoteView from './workspace/NoteView.svelte'

  let { id }: { id: string } = $props()

  applyTheme(() => backend.systemClock24h())
  $effect(() => void setLocale(prefs.language ?? systemLocale()))
  $effect(() => applyUiSize(prefs.uiSize))
  $effect(() => applyDyslexicFont(prefs.dyslexicFont))

  let stage = $state<'loading' | 'unlock' | 'ready'>('loading')
  let status = $state<VaultStatus | null>(null)

  async function open() {
    status = await backend.vaultStatus()
    if (!status.unlocked) {
      stage = 'unlock'
      return
    }
    await workspace.load()
    await workspace.open(id)
    stage = 'ready'
    // other devices' edits come in as in the main window
    void sync.start()
    void localSync.start()
  }

  async function unlocked() {
    void announceUnlocked()
    await open()
  }

  // the note went to the trash or away, or a link led to a note another
  // window has: nothing left to show here
  $effect(() => {
    if (stage === 'ready' && !workspace.current) void getCurrentWindow().destroy()
  })
  // the note window's title follows the note's
  $effect(() => {
    if (workspace.current) document.title = workspace.current.title.trim() || t('note.untitled')
  })

  const stops = [
    onChangedElsewhere(() => {
      if (stage === 'ready') void workspace.remoteChanged()
    }),
    onFlushAsked(() => workspace.flush()),
    onLocked(() => {
      workspace.forget()
      stage = 'loading'
      void open()
    }),
    onUnlockedElsewhere(() => {
      if (stage === 'unlock') void open()
    }),
    // closing saves first; a note that could not be saved keeps its window
    getCurrentWindow().onCloseRequested(async (event) => {
      event.preventDefault()
      await workspace.flush()
      if (!workspace.unsaved) await getCurrentWindow().destroy()
    }),
  ]
  onDestroy(() => stops.forEach((stop) => void stop.then((unlisten) => unlisten())))
  reportActivity()

  void open().finally(() => void windowReady())
</script>

{#if stage === 'unlock' && status}
  <Unlock done={unlocked} {status} />
{:else if stage === 'ready' && workspace.current}
  <div class="note"><NoteView narrow={false} /></div>
{/if}

<Overlays />
<Toasts />

<style>
  .note {
    height: 100%;
    display: flex;
  }
</style>
