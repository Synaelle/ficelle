<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The file viewer's part in the main window: a note imported from a file
     opens here once the notes do, and on Android a file opened with the app
     shows over every screen, the lock screen too, which stays locked under
     it. -->
<script lang="ts">
  import { backend } from '../lib/backend'
  import { ANDROID } from '../lib/platform'
  import { workspace } from '../lib/workspace.svelte'
  import FileWindow from './FileWindow.svelte'
  import { onNoteToOpen, takeNoteToOpen, takeOpenedFile, takeOpenedFiles } from './files'

  let {
    ready,
    unlockedHere,
  }: {
    /** The notes are open in this window. */
    ready: boolean
    /** The viewer's import unlocked the notes: the window opens them. */
    unlockedHere: () => Promise<void>
  } = $props()

  // a file imported in its own window: its note opens here once the notes do
  async function openImported() {
    if (!ready) return
    const id = await takeNoteToOpen().catch(() => null)
    if (id) await workspace.open(id)
  }
  $effect(() => {
    if (ready && backend.native) void openImported()
  })
  if (backend.native) void onNoteToOpen(() => void openImported())

  /** Android: grows for each file opened with the app. */
  let openedFile = $state(0)
  async function openFile() {
    const address = takeOpenedFiles().at(-1)
    if (!address) return
    if (ready) await workspace.flush()
    await takeOpenedFile(address)
    openedFile += 1
  }
  /** Back from the file: an import there may have unlocked the notes, and
   * asked for its note to open. */
  async function closeFile() {
    openedFile = 0
    if (!ready && (await backend.vaultStatus()).unlocked) await unlockedHere()
    await openImported()
  }
  if (ANDROID) {
    window.__appFile = () => void openFile()
    void openFile()
  }
</script>

{#if openedFile}
  {#key openedFile}
    <div class="opened-file"><FileWindow close={closeFile} /></div>
  {/key}
{/if}

<style>
  .opened-file {
    position: fixed;
    inset: 0;
    z-index: 70;
  }
</style>
