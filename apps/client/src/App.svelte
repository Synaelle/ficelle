<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Root: applies display preferences, then shows first launch, unlock or the notes. -->
<script lang="ts">
  import DevReport from './devreport/DevReport.svelte'
  import { captureErrors } from './devreport/report'
  import { autoLock } from './lib/autolock.svelte'
  import { backend, type VaultStatus } from './lib/backend'
  import { applyDyslexicFont } from './lib/dyslexicFont'
  import { captureLogs } from './lib/feedback'
  import { setLocale, systemLocale } from './lib/i18n.svelte'
  import { persistPrefs, prefs } from './lib/prefs.svelte'
  import { applyQuickShortcut } from './lib/quickNote'
  import Overlays from './lib/Overlays.svelte'
  import CriticalUpdate from './lib/CriticalUpdate.svelte'
  import Toasts from './lib/Toasts.svelte'
  import { appUpdates } from './lib/appUpdate.svelte'
  import { applyTheme } from './lib/theme.svelte'
  import { applyUiSize } from './lib/uiSize'
  import { workspace } from './lib/workspace.svelte'
  import Setup from './screens/Setup.svelte'
  import Unlock from './screens/Unlock.svelte'
  import Workspace from './workspace/Workspace.svelte'

  type Stage = 'loading' | 'setup' | 'unlock' | 'upgrade' | 'ready'
  let stage = $state<Stage>('loading')
  let status = $state<VaultStatus | null>(null)
  /** An old vault the app opened without asking: it gets a password before
   * the notes show. The browser's test vault stays as it is. A vault left
   * unencrypted on purpose stays so, Security offers to encrypt it. */
  const needsPassword = (s: VaultStatus) => backend.native && s.way === 'device'

  if (__APP_DEV_REPORTS__) captureErrors()
  captureLogs()
  persistPrefs()
  applyTheme()
  $effect(() => setLocale(prefs.language ?? systemLocale()))
  $effect(() => applyUiSize(prefs.uiSize))
  $effect(() => applyDyslexicFont(prefs.dyslexicFont))
  $effect(() => void applyQuickShortcut(prefs.quickNoteShortcut))

  async function start() {
    status = await backend.vaultStatus()
    if (!status.exists) return (stage = 'setup')
    if (status.unlocked) return (stage = needsPassword(status) ? 'upgrade' : 'ready')
    // the unlock screen asks for Windows Hello, the fingerprint, the PIN or
    // the password itself
    if (status.way !== 'device' && status.way !== 'plain') return (stage = 'unlock')
    try {
      // old vaults: plain ones open directly, device kept ones with this
      // device's key, then they are given a password
      await backend.vaultUnlock(null)
      stage = needsPassword(status) ? 'upgrade' : 'ready'
    } catch {
      stage = 'unlock'
    }
  }

  /** The notes fade as they lock, the lock screen's knot then ties itself. */
  let closing = $state(false)

  async function lock() {
    closing = true
    await workspace.flush()
    // long enough to see the notes go, short enough not to wait for it
    if (!matchMedia('(prefers-reduced-motion: reduce)').matches)
      await new Promise((resolve) => setTimeout(resolve, 220))
    await backend.vaultLock()
    status = await backend.vaultStatus()
    stage = 'unlock'
    closing = false
  }

  /** Called once unlocked or created, the vault may have become encrypted. */
  async function ready() {
    status = await backend.vaultStatus()
    stage = needsPassword(status) ? 'upgrade' : 'ready'
  }

  // every vault asks for something now; only the browser's test vault and an
  // old one not yet given a password have nothing to lock with
  const lockable = $derived(
    stage === 'ready' && !!status && status.way !== 'device' && status.way !== 'plain',
  )
  $effect(() => {
    if (lockable) autoLock(() => void lock())
  })
  // updates download while the notes are open, never on the lock screen
  $effect(() => {
    appUpdates.shown = stage === 'ready'
    if (stage === 'ready') appUpdates.start()
  })

  void start()
</script>

{#if stage === 'setup'}
  <Setup done={ready} />
{:else if stage === 'upgrade' && status?.way === 'device'}
  <Setup done={ready} upgrade />
{:else if stage === 'unlock' && status}
  <Unlock done={ready} {status} />
{:else if stage === 'ready'}
  <div class="notes" class:closing>
    <Workspace onlock={lock} {lockable} />
  </div>
{/if}

<Overlays />
<Toasts />
<CriticalUpdate />

{#if __APP_DEV_REPORTS__}<DevReport />{/if}

<style>
  .notes {
    height: 100%;
  }
  /* locking: the notes blur and fade away before the lock screen */
  .notes.closing {
    pointer-events: none;
    animation: notes-close 220ms ease-in forwards;
  }
  @keyframes notes-close {
    to {
      opacity: 0;
      filter: blur(6px);
      transform: scale(0.985);
    }
  }
</style>
