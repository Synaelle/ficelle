<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Root: applies display preferences, then shows unlock or the notes. A first
     launch makes the notes, not encrypted, and goes straight to them. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { WEB } from './lib/platform'
  import DevReport from './devreport/DevReport.svelte'
  import BetaFeedback from './lib/BetaFeedback.svelte'
  import WebWelcome from './lib/WebWelcome.svelte'
  import TagsMerged from './workspace/TagsMerged.svelte'
  import { autoLock } from './lib/autolock.svelte'
  import { backend, type VaultStatus } from './lib/backend'
  import { applyDyslexicFont } from './lib/dyslexicFont'
  import { catchErrors, errorCatch } from './lib/errorCatch.svelte'
  import ErrorReport from './lib/ErrorReport.svelte'
  import { encryptFlow } from './lib/encryptFlow.svelte'
  import { modal } from './lib/modal'
  import ChannelSwitch from './lib/ChannelSwitch.svelte'
  import { channelSwitch } from './lib/channelChange.svelte'
  import { captureLogs } from './lib/feedback'
  import { setLocale, systemLocale, t } from './lib/i18n.svelte'
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

  type Stage = 'loading' | 'failed' | 'elsewhere' | 'unlock' | 'upgrade' | 'ready'
  /** Why the notes cannot be opened, when it is known, and their folder. */
  let cannotOpen = $state<{ kind: 'readOnly' | 'vaultDamaged'; folder: string } | null>(null)
  let stage = $state<Stage>('loading')
  let status = $state<VaultStatus | null>(null)
  /** An old vault the app opened without asking: it gets a password before
   * the notes show. The browser's test vault stays as it is. A vault left
   * unencrypted on purpose stays so, Security offers to encrypt it. */
  const needsPassword = (s: VaultStatus) => backend.native && s.way === 'device'

  captureLogs()
  catchErrors()
  persistPrefs()
  applyTheme(() => backend.systemClock24h())
  $effect(() => void setLocale(prefs.language ?? systemLocale()))
  $effect(() => applyUiSize(prefs.uiSize))
  $effect(() => applyDyslexicFont(prefs.dyslexicFont))
  $effect(() => void applyQuickShortcut(prefs.quickNoteShortcut))

  async function start() {
    try {
      status = await backend.vaultStatus()
    } catch (err) {
      const { kind, message } = err as { kind?: string; message?: string }
      // another copy of the app has the notes: this one leaves them alone
      if (kind === 'openElsewhere') return (stage = 'elsewhere')
      // a folder that cannot be written or a damaged vault.json: said as such
      if (kind === 'readOnly' || kind === 'vaultDamaged') {
        cannotOpen = { kind, folder: message ?? '' }
        return (stage = 'failed')
      }
      throw err
    }
    if (!status.exists) return void firstLaunch()
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

  /** Encryption is a choice in Settings, Security, not a question on the way in. */
  async function firstLaunch() {
    stage = 'loading'
    try {
      await backend.vaultCreate('plain', '', null)
      await ready()
    } catch {
      stage = 'failed'
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

{#if stage === 'failed' && cannotOpen}
  <main class="failed">
    <h1>{t('unlock.failedTitle')}</h1>
    <p role="alert">
      {cannotOpen.kind === 'readOnly'
        ? t('unlock.readOnlyHelp', { app: brand.displayName })
        : t('unlock.damagedHelp')}
    </p>
    <p class="folder">{cannotOpen.folder}</p>
    <button
      class="btn primary"
      onclick={() => ((cannotOpen = null), (stage = 'loading'), void start())}
      >{t('unlock.retry')}</button
    >
  </main>
{:else if stage === 'failed'}
  <main class="failed">
    <h1>{t('unlock.failedTitle')}</h1>
    <p role="alert">{t('unlock.failedHelp')}</p>
    <button class="btn primary" onclick={firstLaunch}>{t('unlock.retry')}</button>
  </main>
{:else if stage === 'elsewhere'}
  <main class="failed">
    {#if WEB}
      <h1>{t('unlock.elsewhereTabTitle')}</h1>
      <p role="alert">{t('unlock.elsewhereTabHelp', { app: brand.displayName })}</p>
    {:else}
      <h1>{t('unlock.elsewhereTitle', { app: brand.displayName })}</h1>
      <p role="alert">{t('unlock.elsewhereHelp', { app: brand.displayName })}</p>
    {/if}
    <!-- in the web version a second try takes the notes from the other tab -->
    <button class="btn primary" onclick={() => ((stage = 'loading'), void start())}
      >{WEB ? t('unlock.useHere') : t('unlock.retry')}</button
    >
  </main>
{:else if stage === 'upgrade' && status?.way === 'device'}
  <Setup done={ready} />
{:else if stage === 'unlock' && status}
  <Unlock done={ready} {status} />
{:else if stage === 'ready'}
  <div class="notes" class:closing>
    <Workspace onlock={lock} {lockable} />
  </div>
  <BetaFeedback />
  <TagsMerged />
  {#if WEB && !prefs.webWelcomed}<WebWelcome />{/if}
{/if}

<Overlays />
<Toasts />
<CriticalUpdate />
{#if errorCatch.open}<ErrorReport />{/if}
{#if channelSwitch.open}<ChannelSwitch />{/if}
{#if encryptFlow.open}
  <!-- a dialog of its own, over the settings: the keys and the focus are its -->
  <div
    class="encrypting"
    role="dialog"
    aria-modal="true"
    aria-label={t('setup.encryptTitle')}
    use:modal={{ onclose: () => encryptFlow.finish(false) }}
  >
    <!-- once encrypted the notes can lock: Lock and the auto lock read the new state -->
    <Setup
      encrypt
      done={() => {
        encryptFlow.finish(true)
        void ready()
      }}
      cancel={() => encryptFlow.finish(false)}
    />
  </div>
{/if}

{#if __APP_DEV_TOOLS__}<DevReport />{/if}

<style>
  .failed {
    display: grid;
    align-content: center;
    justify-items: center;
    gap: var(--space-3);
    height: 100%;
    padding: var(--space-4);
    text-align: center;
  }
  .failed h1 {
    margin: 0;
    font-size: 22px;
  }
  .failed p {
    margin: 0;
    max-width: 52ch;
    color: var(--color-text-muted);
  }
  .failed .folder {
    font-family: var(--font-mono);
    font-size: 13px;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .notes {
    height: 100%;
  }
  /* the setup's password step over everything, the settings included */
  .encrypting {
    position: fixed;
    inset: 0;
    z-index: 80;
    overflow-y: auto;
    background: var(--color-bg);
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
