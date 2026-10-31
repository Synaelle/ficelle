<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Settings dialog. A search box filters sections by their translated text.
     Phones get their own full screen layout, PhoneSettings. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import { brand } from '@app/brand'
  import { modal } from '../lib/modal'
  import { onMount } from 'svelte'
  import { backend } from '../lib/backend'
  import { biometricMethod } from '../lib/biometric'
  import { LOCALES, t, type Locale } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { prefs, shortcutFor } from '../lib/prefs.svelte'
  import {
    applyQuickShortcut,
    DEFAULT_QUICK_SHORTCUT,
    globalBindingOf,
    quickNoteSupported,
  } from '../lib/quickNote'
  import { ACTIONS, bindingOf, displayBinding, type ActionId } from '../lib/shortcuts'
  import RecoveryKey from '../screens/RecoveryKey.svelte'
  import AppearancePickers from './AppearancePickers.svelte'
  import LayoutPicker from './LayoutPicker.svelte'
  import BackupDialog from './BackupDialog.svelte'
  import ResetDialog from './ResetDialog.svelte'
  import BackupSettings from './BackupSettings.svelte'
  import ImportDialog from './ImportDialog.svelte'
  import FeedbackSettings from './FeedbackSettings.svelte'
  import PhoneSettings from './PhoneSettings.svelte'
  import SizePicker from './SizePicker.svelte'
  import SyncSettings from './SyncSettings.svelte'
  import UpdateCheck from './UpdateCheck.svelte'
  import TestTools from '../devreport/TestTools.svelte'
  import ContactLine from './ContactLine.svelte'
  import { serverProtocol } from '../lib/sync.svelte'
  import { updatesHere } from '../lib/appUpdate.svelte'
  import { dyslexicStatus } from '../lib/dyslexicStatus.svelte'
  import { LOCK_CHOICES, lockLabel, SettingsState, ways } from './settingsState.svelte'

  let { close, onlock, start }: { close: () => void; onlock: () => void; start?: 'sync' } = $props()

  type Section =
    | 'appearance'
    | 'accessibility'
    | 'editor'
    | 'language'
    | 'shortcuts'
    | 'security'
    | 'sync'
    | 'data'
    | 'backups'
    | 'feedback'
    | 'experimental'
    | 'testTools'
    | 'about'
  const SECTIONS: Section[] = [
    'appearance',
    'accessibility',
    'editor',
    'language',
    'shortcuts',
    'security',
    'sync',
    'data',
    'backups',
    'feedback',
    'experimental',
    // dev and preview builds only
    ...(__APP_DEV_REPORTS__ ? (['testTools'] as const) : []),
    'about',
  ] as Section[]
  const SECTION_ICONS: Record<Section, IconName> = {
    appearance: 'palette',
    accessibility: 'textSize',
    editor: 'textStyle',
    language: 'language',
    shortcuts: 'keyboard',
    security: 'lock',
    sync: 'synced',
    data: 'database',
    backups: 'backup',
    feedback: 'feedback',
    experimental: 'flask',
    testTools: 'log',
    about: 'info',
  }
  /** At phone width the settings fill the screen with their own layout. */
  const PHONE = 640
  let width = $state(window.innerWidth)
  const phone = $derived(width <= PHONE)
  // svelte-ignore state_referenced_locally
  let section: Section = $state(start ?? 'appearance')
  /** The linked server's protocol versions, asked when About opens. */
  let serverProtocols: string | null = $state(null)
  $effect(() => {
    if (section === 'about') void serverProtocol().then((v) => (serverProtocols = v))
  })
  /** The import dialog open, for this app's own export or another app's. */
  let importing = $state<'ficelle' | 'other' | null>(null)
  let resetting = $state(false)
  let backingUp = $state<'make' | 'restore' | null>(null)
  let filter = $state('')
  const visible = $derived(
    SECTIONS.filter(
      (s) =>
        !filter.trim() ||
        t(`settings.${s}`).toLowerCase().includes(filter.toLowerCase()) ||
        t(`settings.${s}Keywords`).toLowerCase().includes(filter.toLowerCase()),
    ),
  )

  const settings = new SettingsState()
  onMount(() => void settings.refresh())
  const method = $derived(biometricMethod())

  let recording: ActionId | null = $state(null)
  let recordingQuick = $state(false)
  let quickTaken = $state(false)

  /** The system may refuse a global shortcut another app holds. */
  async function setQuick(binding: string | null) {
    quickTaken = false
    if (await applyQuickShortcut(binding)) prefs.quickNoteShortcut = binding
    else {
      quickTaken = true
      await applyQuickShortcut(prefs.quickNoteShortcut)
    }
  }

  function record(event: KeyboardEvent) {
    if (recordingQuick) {
      event.preventDefault()
      event.stopPropagation()
      if (event.key === 'Escape') return (recordingQuick = false)
      const binding = globalBindingOf(event)
      if (!binding) return
      recordingQuick = false
      void setQuick(binding)
      return
    }
    if (!recording) return
    event.preventDefault()
    event.stopPropagation()
    if (event.key === 'Escape') return (recording = null)
    const binding = bindingOf(event)
    if (!binding) return
    prefs.shortcuts = { ...prefs.shortcuts, [recording]: binding }
    recording = null
  }

  let oldPassword = $state('')
  let newPassword = $state('')
  async function changePassword(event: SubmitEvent) {
    event.preventDefault()
    if (await settings.changePassword(oldPassword, newPassword)) oldPassword = newPassword = ''
  }
</script>

<svelte:window onkeydowncapture={record} bind:innerWidth={width} />

{#if phone}
  <PhoneSettings
    {settings}
    {close}
    {onlock}
    {start}
    onimport={(ficelle) => (importing = ficelle ? 'ficelle' : 'other')}
    onbackup={(mode) => (backingUp = mode)}
    onreset={() => (resetting = true)}
  />
{:else}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && close()}
  >
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-label={t('settings.title')}
      use:modal={{ onclose: close }}
    >
      <nav>
        <input class="field" type="search" placeholder={t('settings.search')} bind:value={filter} />
        {#each visible as s (s)}
          <button
            class:active={section === s}
            onclick={() => ((section = s), (settings.message = ''))}
            ><Icon name={SECTION_ICONS[s]} size={16} />{t(`settings.${s}`)}</button
          >
        {/each}
      </nav>

      <div class="content">
        <header>
          <h2>{t(`settings.${section}`)}</h2>
          <button class="icon-btn" aria-label={t('common.close')} onclick={close}
            ><Icon name="close" /></button
          >
        </header>

        <div class="settings-body">
          {#if section === 'appearance'}
            <AppearancePickers layout={false} />
          {:else if section === 'accessibility'}
            <SizePicker />
            <div class="settings-group">
              <label class="settings-check">
                <input type="checkbox" bind:checked={prefs.dyslexicFont} data-dyslexic-switch />
                {t('settings.dyslexicFont')}
              </label>
              {#if prefs.dyslexicFont && dyslexicStatus.loading}
                <p class="settings-help" role="status">{t('settings.dyslexicFontLoading')}</p>
              {:else if prefs.dyslexicFont && dyslexicStatus.failed}
                <p role="alert">{t('settings.dyslexicFontFailed')}</p>
              {/if}
            </div>
          {:else if section === 'editor'}
            <div class="settings-group">
              <label class="settings-check">
                <input type="checkbox" bind:checked={prefs.autoLists} />
                {t('settings.autoLists')}
              </label>
              <label class="settings-check">
                <input type="checkbox" bind:checked={prefs.emojiShortcodes} />
                {t('settings.emojiShortcodes')}
              </label>
              <label class="settings-check">
                <input type="checkbox" bind:checked={prefs.openNoteFirst} />
                {t('settings.openNoteFirst')}
              </label>
            </div>
          {:else if section === 'language'}
            <div class="settings-group">
              <label class="settings-field">
                {t('settings.language')}
                <select
                  class="field"
                  value={prefs.language ?? ''}
                  onchange={(e) =>
                    (prefs.language = (e.currentTarget.value || null) as Locale | null)}
                >
                  <option value="">{t('settings.languageSystem')}</option>
                  {#each LOCALES as locale (locale.code)}
                    <option value={locale.code}>{locale.name}</option>
                  {/each}
                </select>
              </label>
              <label class="settings-field">
                {t('settings.clock')}
                <select class="field" bind:value={prefs.clock}>
                  {#each ['system', '24', '12'] as const as choice (choice)}
                    <option value={choice}>{t(`settings.clock_${choice}`)}</option>
                  {/each}
                </select>
              </label>
            </div>
          {:else if section === 'shortcuts'}
            <div class="settings-group wide">
              <ul class="shortcuts">
                {#each ACTIONS as action (action)}
                  <li>
                    <span>{t(`shortcuts.${action}`)}</span>
                    <button class="btn key" onclick={() => (recording = action)}>
                      {recording === action
                        ? t('settings.pressKeys')
                        : displayBinding(shortcutFor(action))}
                    </button>
                  </li>
                {/each}
              </ul>
              <div class="settings-actions">
                <button class="btn" onclick={() => (prefs.shortcuts = {})}
                  >{t('settings.resetShortcuts')}</button
                >
              </div>
            </div>
            {#if quickNoteSupported}
              <div class="settings-group">
                <h3>{t('settings.quickNote')}</h3>
                <div class="settings-actions">
                  <label class="settings-check">
                    <input
                      type="checkbox"
                      aria-label={t('settings.quickNote')}
                      checked={prefs.quickNoteShortcut !== null}
                      onchange={(e) =>
                        void setQuick(e.currentTarget.checked ? DEFAULT_QUICK_SHORTCUT : null)}
                    />
                    {prefs.quickNoteShortcut ? '' : t('settings.quickNoteOff')}
                  </label>
                  {#if prefs.quickNoteShortcut}
                    <button class="btn key" onclick={() => (recordingQuick = true)}>
                      {recordingQuick
                        ? t('settings.pressKeys')
                        : displayBinding(prefs.quickNoteShortcut)}
                    </button>
                  {/if}
                </div>
                {#if quickTaken}<p role="alert">{t('settings.quickNoteTaken')}</p>{/if}
              </div>
            {/if}
          {:else if section === 'security'}
            {@const vault = settings.vault}
            {#if vault && !vault.encrypted}
              <div class="settings-group">
                <h3>{t('settings.notEncryptedTitle')}</h3>
                <div class="settings-actions">
                  <button class="btn primary" onclick={() => settings.encrypt()}
                    ><Icon name="lock" size={16} />{t('settings.encrypt')}</button
                  >
                </div>
              </div>
            {/if}
            {#if vault?.encrypted}
              <div class="settings-group">
                <h3>{t('settings.wayTitle')}</h3>
                <div class="ways" role="radiogroup" aria-label={t('settings.wayTitle')}>
                  {#each ways(vault) as option (option)}
                    <label class="way" class:current={vault.way === option}>
                      <input
                        type="radio"
                        name="way"
                        checked={vault.way === option}
                        onchange={() => settings.setWay(option)}
                      />
                      <span>
                        <strong>{t(`settings.way.${option}`, { method })}</strong>
                        <span class="settings-help"
                          >{t(`settings.wayHelp.${option}`, { method })}</span
                        >
                      </span>
                    </label>
                  {/each}
                </div>
                {#if vault.way === 'pin'}
                  <div class="settings-actions">
                    <button class="btn" onclick={() => settings.setWay('pin')}
                      >{t('pin.change')}</button
                    >
                  </div>
                {/if}
              </div>
              {#if vault.way === 'guarded' || vault.way === 'pin' || vault.way === 'password'}
                <div class="settings-group">
                  <h3>{t('settings.autoLock')}</h3>
                  <select
                    class="field"
                    aria-label={t('settings.autoLock')}
                    bind:value={prefs.autoLockMinutes}
                  >
                    {#each LOCK_CHOICES as minutes (minutes)}
                      <option value={minutes}>{lockLabel(minutes)}</option>
                    {/each}
                  </select>
                  <div class="settings-actions">
                    <button class="btn" onclick={onlock}
                      ><Icon name="lock" size={16} />{t('sidebar.lock')}</button
                    >
                  </div>
                </div>
              {/if}
              <details class="settings-group more" open={!!settings.recoveryCode}>
                <summary>{t('recovery.heading')}</summary>
                {#if settings.recoveryCode}
                  <RecoveryKey
                    code={settings.recoveryCode}
                    done={() => settings.recoveryShown()}
                    level="h3"
                  />
                {:else}
                  <p class="settings-help">
                    {vault.recovery ? t('recovery.has') : t('recovery.noneNow')}
                  </p>
                  <div class="settings-actions">
                    <button class="btn" onclick={() => settings.createRecovery()}
                      >{vault.recovery ? t('recovery.replace') : t('recovery.create')}</button
                    >
                    {#if vault.recovery}
                      <button class="btn danger" onclick={() => settings.removeRecovery()}
                        >{t('recovery.remove')}</button
                      >
                    {/if}
                  </div>
                {/if}
              </details>
              {#if vault.hasPassword}
                <details class="settings-group more">
                  <summary>{t('settings.changePassword')}</summary>
                  <form class="settings-group" onsubmit={changePassword}>
                    <input
                      type="text"
                      autocomplete="username"
                      value={brand.displayName}
                      hidden
                      readonly
                    />
                    <PasswordField
                      class="field"
                      autocomplete="current-password"
                      placeholder={t('settings.oldPassword')}
                      bind:value={oldPassword}
                    />
                    <PasswordField
                      class="field"
                      autocomplete="new-password"
                      placeholder={t('settings.newPassword')}
                      bind:value={newPassword}
                    />
                    <div class="settings-actions">
                      <button
                        class="btn"
                        type="submit"
                        disabled={!oldPassword || newPassword.length < 8}
                        >{t('settings.changePassword')}</button
                      >
                    </div>
                  </form>
                </details>
              {/if}
            {/if}
          {:else if section === 'sync'}
            <SyncSettings />
          {:else if section === 'backups'}
            <BackupSettings onbackup={(mode) => (backingUp = mode)} />
          {:else if section === 'data'}
            <div class="settings-group">
              <h3>{t('settings.importTitle')}</h3>
              <p class="settings-help">{t('settings.importHelp')}</p>
              <div class="settings-actions">
                <button
                  class="btn"
                  onclick={() => (importing = 'ficelle')}
                  disabled={!backend.native}
                  ><Icon name="import" size={16} />{t('settings.importFicelle', {
                    app: brand.displayName,
                  })}</button
                >
                <button class="btn" onclick={() => (importing = 'other')} disabled={!backend.native}
                  >{t('settings.import')}</button
                >
              </div>
            </div>
            <div class="settings-group">
              <h3>{t('settings.exportTitle')}</h3>
              <p class="settings-help">{t('settings.exportHelp')}</p>
              <div class="settings-actions">
                <button
                  class="btn"
                  onclick={() => settings.exportNotes()}
                  disabled={!backend.native}
                  ><Icon name="export" size={16} />{t('settings.export')}</button
                >
                <button class="btn" onclick={() => settings.exportZip()} disabled={!backend.native}
                  >{t('settings.exportZip')}</button
                >
              </div>
            </div>
            <details class="settings-group more">
              <summary>{t('reset.title')}</summary>
              <p class="settings-help">{t('reset.help')}</p>
              <div class="settings-actions">
                <button class="btn danger" onclick={() => (resetting = true)}
                  >{t('reset.open')}</button
                >
              </div>
            </details>
          {:else if section === 'experimental'}
            <p class="settings-help">{t('settings.experimentalHelp')}</p>
            <LayoutPicker />
          {:else if section === 'testTools'}
            <TestTools />
          {:else if section === 'feedback'}
            <FeedbackSettings />
          {:else if section === 'about'}
            <div class="settings-group">
              <dl>
                <dt>{t('settings.version')}</dt>
                <dd>{__APP_VERSION__}</dd>
                <dt>{t('settings.buildDate')}</dt>
                <dd>{__APP_BUILD_DATE__}</dd>
                <dt>{t('settings.protocol')}</dt>
                <dd>{__APP_PROTOCOL__}</dd>
                {#if serverProtocols}
                  <dt>{t('settings.serverProtocol')}</dt>
                  <dd>{serverProtocols}</dd>
                {/if}
              </dl>
            </div>
            <div class="settings-group"><ContactLine /></div>
            {#if updatesHere}
              <div class="settings-group">
                <h3>{t('appUpdate.title')}</h3>
                <UpdateCheck />
              </div>
            {/if}
          {/if}
        </div>

        {#if settings.message}<p class="message" role="status">{settings.message}</p>{/if}
      </div>
    </div>
  </div>
{/if}

{#if resetting}
  <ResetDialog close={() => (resetting = false)} />
{/if}

{#if importing}
  <ImportDialog ficelle={importing === 'ficelle'} close={() => (importing = null)} />
{/if}

{#if backingUp}
  <BackupDialog mode={backingUp} close={() => (backingUp = null)} />
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: color-mix(in srgb, var(--color-bg) 70%, transparent);
  }
  .dialog {
    width: min(820px, 100%);
    height: min(620px, 100%);
    display: grid;
    grid-template-columns: 200px 1fr;
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-window);
    overflow: hidden;
  }
  nav {
    display: grid;
    align-content: start;
    gap: 2px;
    padding: var(--space-3);
    background: var(--color-bg);
    border-right: 1px solid var(--color-border);
  }
  nav input {
    margin-bottom: var(--space-2);
  }
  nav button {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 32px;
    padding: 0 var(--space-2);
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    text-align: left;
  }
  nav button.active {
    background: var(--color-selection);
  }
  .content {
    overflow-y: auto;
    padding: 0 var(--space-5) var(--space-5);
  }
  header {
    position: sticky;
    top: 0;
    /* above the section's own layered parts, the theme cards among them */
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) 0;
    background: var(--color-surface);
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  .shortcuts {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .shortcuts li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--color-border);
  }
  .key {
    min-width: 110px;
    justify-content: center;
    font-family: var(--font-mono);
  }
  dl {
    margin: 0;
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-2) var(--space-4);
  }
  .message {
    margin: var(--space-5) 0 0;
  }
  dd {
    margin: 0;
    font-family: var(--font-mono);
  }
  .ways {
    display: grid;
    gap: var(--space-2);
  }
  .way {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    cursor: pointer;
  }
  .way.current {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 7%, transparent);
  }
  .way input {
    margin-top: 3px;
  }
  .way > span {
    display: grid;
    gap: 2px;
  }
</style>
