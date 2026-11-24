<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Root: applies display preferences, then shows unlock or the notes. A first
     launch makes the notes, not encrypted, and asks about backups and
     encryption before them; the web version goes straight to them. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { ANDROID, LINUX, PHONE, WEB } from './lib/platform'
  import { followBrowserBack } from './lib/browserBack.svelte'
  // file viewer
  import FileViewer from './fileViewer/InApp.svelte'
  import DevReport from './devreport/DevReport.svelte'
  import BetaFeedback from './lib/BetaFeedback.svelte'
  import WebWelcome from './lib/WebWelcome.svelte'
  import TagsMerged from './workspace/TagsMerged.svelte'
  import { autoLock, liftCoverOnReturn } from './lib/autolock.svelte'
  import { backend, type VaultStatus } from './lib/backend'
  import { reopenNotes } from './lib/backend/web'
  import { storageTrouble, type StorageTrouble } from './lib/storageTrouble'
  import { storageRefused } from './lib/webStorage'
  import { applyDyslexicFont } from './lib/dyslexicFont'
  import { catchErrors, errorCatch } from './lib/errorCatch.svelte'
  import { errorCode, shownError, type ShownError } from './lib/errors'
  import { followPageParts } from './lib/pageParts'
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
  import { toasts } from './lib/toast.svelte'
  import { appUpdates } from './lib/appUpdate.svelte'
  import { applyTheme } from './lib/theme.svelte'
  import { applyUiSize } from './lib/uiSize'
  import { workspace } from './lib/workspace.svelte'
  import {
    announceUnlocked,
    flushNoteWindows,
    followActivity,
    noteWindowsSupported,
    onChangedElsewhere,
    onLocked,
    onUnlockedElsewhere,
  } from './lib/windows'
  import Setup from './screens/Setup.svelte'
  import Unlock from './screens/Unlock.svelte'
  import Welcome from './screens/Welcome.svelte'

  type Stage = 'loading' | 'failed' | 'elsewhere' | 'unlock' | 'upgrade' | 'welcome' | 'ready'
  /** Why the notes cannot be opened, when it is known, and their folder. */
  let cannotOpen = $state<{ kind: CannotOpen; folder: string } | null>(null)
  /** What keeps the notes from opening, with its help text. */
  const CANNOT_OPEN = {
    readOnly: 'unlock.readOnlyHelp',
    vaultDamaged: 'unlock.damagedHelp',
    notesMissing: 'unlock.notesMissingHelp',
    vaultMissing: 'unlock.vaultMissingHelp',
  }
  type CannotOpen = keyof typeof CANNOT_OPEN
  const cannotOpenKind = (kind: string | undefined): kind is CannotOpen =>
    !!kind && Object.hasOwn(CANNOT_OPEN, kind)
  let stage = $state<Stage>('loading')
  /** In the web version, what kept the browser's storage from the notes. */
  let trouble = $state<StorageTrouble | null>(null)
  /** Why the notes did not open, with its code to send back. */
  let failure = $state<ShownError | null>(null)
  /** What the browser's storage did, as an error. */
  const troubleError = (found: StorageTrouble): ShownError => ({
    kind: `storage_${found}`,
    reason: t(`web.storage_${found}`),
    code: errorCode('START', `storage_${found}`),
  })
  let status = $state<VaultStatus | null>(null)
  /** An old vault the app opened without asking: it gets a password before
   * the notes show. The browser's test vault stays as it is. A vault left
   * unencrypted on purpose stays so, Security offers to encrypt it. */
  const needsPassword = (s: VaultStatus) => backend.native && s.way === 'device'

  captureLogs()
  catchErrors()
  if (WEB) followPageParts()
  if (WEB && PHONE) followBrowserBack()
  if (ANDROID) liftCoverOnReturn()
  persistPrefs()
  applyTheme(() => backend.systemClock24h())
  $effect(() => void setLocale(prefs.language ?? systemLocale()))
  $effect(() => applyUiSize(prefs.uiSize))
  $effect(() => applyDyslexicFont(prefs.dyslexicFont))
  // Linux's web view draws text thinner than Windows, where the look was
  // made: every stroke 0.2 px thicker gives a paragraph the ink it has there.
  // An experimental setting for now, shown on the nightly channel only
  $effect(() => {
    const thicker = LINUX && !WEB && prefs.channel === 'nightly' && prefs.windowsText
    document.documentElement.style.setProperty('--text-stroke', thicker ? '0.2px' : '0px')
  })
  $effect(() => void applyQuickShortcut(prefs.quickNoteShortcut))

  /** Waits before each new try while the page before a reload still has
   * the notes: a big note it was saving can take a few seconds. */
  const BUSY_WAITS = [250, 500, 1000, 2000, 3000]

  async function start(tries = 0) {
    // the browser keeps nothing for this site: no notes could last a reload
    if (WEB && storageRefused) {
      trouble = 'refused'
      failure = troubleError(trouble)
      return (stage = 'failed')
    }
    try {
      status = await backend.vaultStatus()
    } catch (err) {
      const { kind, message } = err as { kind?: string; message?: string }
      // another copy of the app has the notes: this one leaves them alone
      if (kind === 'openElsewhere') return (stage = 'elsewhere')
      // a folder that cannot be written, a damaged or missing vault.json or
      // a missing notes.db: said as such
      if (cannotOpenKind(kind)) {
        cannotOpen = { kind, folder: message ?? '' }
        return (stage = 'failed')
      }
      // the browser's storage would not open: said why, with a way to retry
      if (WEB && kind === 'storage') {
        const found = await storageTrouble(err)
        if (found === 'busy' && tries < BUSY_WAITS.length) {
          await reopenNotes()
          await new Promise((resolve) => setTimeout(resolve, BUSY_WAITS[tries]))
          return start(tries + 1)
        }
        trouble = found
        failure = found ? troubleError(found) : shownError(err, 'START')
        return (stage = 'failed')
      }
      throw err
    }
    // said once, by the first check after the crash
    if (status.encryptionUndone) toasts.push(t('settings.security'), t('settings.encryptionUndone'))
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

  /** The notes are made unencrypted, the welcome then asks. The web version
   * has no automatic backups and its own welcome over the notes. */
  async function firstLaunch() {
    stage = 'loading'
    try {
      await backend.vaultCreate('plain', '', null)
      if (WEB) await ready()
      else stage = 'welcome'
    } catch (err) {
      if (WEB) trouble = await storageTrouble(err)
      failure = trouble ? troubleError(trouble) : shownError(err, 'START')
      stage = 'failed'
    }
  }

  /** The web version starts over with a new worker, which opens the storage
   * again: the one that failed keeps failing, whatever the browser allowed
   * since. */
  async function retry() {
    if (!WEB) return void firstLaunch()
    // what the browser allows is read as the page loads
    if (storageRefused) return location.reload()
    trouble = null
    failure = null
    stage = 'loading'
    await reopenNotes()
    await start()
  }

  /** The notes fade as they lock, the lock screen's knot then ties itself. */
  let closing = $state(false)
  /** Locked as the app showed again: the notes go at once, never seen. */
  let gone = $state(false)

  async function lock(shown = false) {
    if (shown) gone = true
    closing = true
    await workspace.flush()
    await flushNoteWindows()
    // long enough to see the notes go, short enough not to wait for it
    if (!gone && !matchMedia('(prefers-reduced-motion: reduce)').matches)
      await new Promise((resolve) => setTimeout(resolve, 220))
    await backend.vaultLock()
    status = await backend.vaultStatus()
    stage = 'unlock'
    closing = false
    gone = false
  }

  /** Called once unlocked or created, the vault may have become encrypted. */
  async function ready() {
    status = await backend.vaultStatus()
    stage = needsPassword(status) ? 'upgrade' : 'ready'
    if (stage === 'ready') void announceUnlocked()
  }

  // the other windows of the app: a note window's edits show in the lists,
  // its typing keeps the auto-lock away, and every window locks and unlocks
  // together
  if (noteWindowsSupported) {
    void onChangedElsewhere(() => {
      if (stage === 'ready') void workspace.remoteChanged()
    })
    void followActivity()
    void onLocked(async () => {
      if (stage !== 'ready' || closing) return
      status = await backend.vaultStatus()
      if (!status.unlocked) stage = 'unlock'
    })
    void onUnlockedElsewhere(async () => {
      if (stage !== 'unlock') return
      status = await backend.vaultStatus()
      if (status.unlocked) await ready()
    })
  }

  // every vault asks for something now; only the browser's test vault and an
  // old one not yet given a password have nothing to lock with
  const lockable = $derived(
    stage === 'ready' && !!status && status.way !== 'device' && status.way !== 'plain',
  )
  $effect(() => {
    if (lockable) autoLock((shown) => void lock(shown))
  })
  // updates download while the notes are open, never on the lock screen
  $effect(() => {
    appUpdates.shown = stage === 'ready'
    if (stage === 'ready') void appUpdates.start()
  })

  // the notes' screen loads while the unlock screen shows, not before it
  const workspaceScreen = import('./workspace/Workspace.svelte')

  void start()
</script>

{#if stage === 'failed' && cannotOpen}
  <main class="failed">
    <h1>{t('unlock.failedTitle')}</h1>
    <p role="alert">
      {t(CANNOT_OPEN[cannotOpen.kind], { app: brand.displayName })}
    </p>
    <p class="folder">{cannotOpen.folder}</p>
    <p class="code">{t('common.errorCode', { code: errorCode('START', cannotOpen.kind) })}</p>
    <button
      class="btn primary"
      onclick={() => ((cannotOpen = null), (stage = 'loading'), void start())}
      >{t('unlock.retry')}</button
    >
  </main>
{:else if stage === 'failed'}
  <main class="failed">
    <h1>{t('unlock.failedTitle')}</h1>
    <p role="alert">{failure?.reason}</p>
    {#if !trouble}<p>{t('unlock.failedHelp')}</p>{/if}
    {#if failure}<p class="code">{t('common.errorCode', { code: failure.code })}</p>{/if}
    <button class="btn primary" onclick={retry}>{t('unlock.retry')}</button>
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
{:else if stage === 'welcome'}
  <Welcome done={ready} />
{:else if stage === 'ready'}
  <div class="notes" class:closing class:gone>
    {#await workspaceScreen then { default: Workspace }}
      <Workspace onlock={() => lock()} {lockable} />
    {/await}
  </div>
  <BetaFeedback />
  <TagsMerged />
  {#if WEB && !prefs.webWelcomed}<WebWelcome />{/if}
{/if}

<!-- file viewer -->
<FileViewer
  ready={stage === 'ready'}
  unlockedHere={async () => {
    if (stage === 'unlock') await ready()
  }}
/>

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
      onencrypted={() => {
        encryptFlow.encryptedNow()
        void ready()
      }}
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
    font-size: var(--title-screen);
  }
  .failed p {
    margin: 0;
    max-width: 52ch;
    color: var(--color-text-muted);
  }
  .failed .code {
    font-size: 13px;
    user-select: text;
  }
  .failed .folder {
    font-family: var(--font-ui);
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
  .notes.gone {
    visibility: hidden;
  }
  @keyframes notes-close {
    to {
      opacity: 0;
      filter: blur(6px);
      transform: scale(0.985);
    }
  }
</style>
