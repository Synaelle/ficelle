<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Settings dialog. A search box filters sections by their translated text.
     Phones get their own full screen layout, PhoneSettings. -->
<script lang="ts">
  import { MIN_PASSWORD } from '../lib/passwords'
  import { workspace } from '../lib/workspace.svelte'
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
  import ArrowPicker from './ArrowPicker.svelte'
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
  import CreditsList from './CreditsList.svelte'
  import { serverProtocol } from '../lib/sync.svelte'
  import { updatesHere } from '../lib/appUpdate.svelte'
  import { dyslexicStatus } from '../lib/dyslexicStatus.svelte'
  import { LOCK_CHOICES, lockLabel, SettingsState, ways } from './settingsState.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Switch from './ui/Switch.svelte'

  let {
    close,
    onlock,
    start,
  }: { close: () => void; onlock: () => void; start?: 'sync' | 'experimental' } = $props()

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
    ...(__APP_DEV_TOOLS__ ? (['testTools'] as const) : []),
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
  let markdownInput: HTMLInputElement | undefined = $state()
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
  /** The credits, open under their row in About. */
  let creditsOpen = $state(false)
  /** The change password form, open under its row. */
  let changingPassword = $state(false)
  async function changePassword(event: SubmitEvent) {
    event.preventDefault()
    if (await settings.changePassword(oldPassword, newPassword)) {
      oldPassword = newPassword = ''
      changingPassword = false
    }
  }
</script>

<svelte:window onkeydowncapture={record} bind:innerWidth={width} />

{#if phone}
  <PhoneSettings
    {settings}
    {close}
    {onlock}
    start={start === 'sync' ? start : undefined}
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
            ><span class="tile"><Icon name={SECTION_ICONS[s]} size={14} /></span>{t(
              `settings.${s}`,
            )}</button
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
            <Group
              note={prefs.dyslexicFont && dyslexicStatus.loading
                ? t('settings.dyslexicFontLoading')
                : undefined}
            >
              <Row label={t('settings.uiSize')} help={t('settings.uiSizeHelp')}>
                <SizePicker />
              </Row>
              <Row label={t('settings.dyslexicFont')}>
                <Switch label={t('settings.dyslexicFont')} bind:checked={prefs.dyslexicFont} />
              </Row>
            </Group>
            {#if prefs.dyslexicFont && dyslexicStatus.failed}
              <p class="settings-help failed" role="alert">{t('settings.dyslexicFontFailed')}</p>
            {/if}
          {:else if section === 'editor'}
            <Group>
              <Row label={t('settings.autoLists')}>
                <Switch label={t('settings.autoLists')} bind:checked={prefs.autoLists} />
              </Row>
              <Row label={t('settings.emojiShortcodes')}>
                <Switch
                  label={t('settings.emojiShortcodes')}
                  bind:checked={prefs.emojiShortcodes}
                />
              </Row>
              <Row label={t('settings.spellcheck')}>
                <Switch label={t('settings.spellcheck')} bind:checked={prefs.spellcheck} />
              </Row>
            </Group>
            <Group title={t('settings.arrow')} typed>
              <div class="inside"><ArrowPicker /></div>
            </Group>
            <!-- how the app behaves, apart from what typing does -->
            <Group note={quickTaken ? t('settings.quickNoteTaken') : undefined}>
              <Row label={t('settings.openNoteFirst')}>
                <Switch label={t('settings.openNoteFirst')} bind:checked={prefs.openNoteFirst} />
              </Row>
              {#if quickNoteSupported}
                <Row label={t('settings.quickNote')}>
                  {#if prefs.quickNoteShortcut}
                    <button class="btn key" onclick={() => (recordingQuick = true)}>
                      {recordingQuick
                        ? t('settings.pressKeys')
                        : displayBinding(prefs.quickNoteShortcut)}
                    </button>
                  {/if}
                  <Switch
                    label={t('settings.quickNote')}
                    checked={prefs.quickNoteShortcut !== null}
                    onchange={(on) => void setQuick(on ? DEFAULT_QUICK_SHORTCUT : null)}
                  />
                </Row>
              {/if}
            </Group>
          {:else if section === 'language'}
            <Group>
              <Row label={t('settings.language')}>
                <select
                  class="field compact"
                  aria-label={t('settings.language')}
                  value={prefs.language ?? ''}
                  onchange={(e) =>
                    (prefs.language = (e.currentTarget.value || null) as Locale | null)}
                >
                  <option value="">{t('settings.languageSystem')}</option>
                  {#each LOCALES as locale (locale.code)}
                    <option value={locale.code}>{locale.name}</option>
                  {/each}
                </select>
              </Row>
              <Row label={t('settings.clock')}>
                <select
                  class="field compact"
                  aria-label={t('settings.clock')}
                  bind:value={prefs.clock}
                >
                  {#each ['system', '24', '12'] as const as choice (choice)}
                    <option value={choice}>{t(`settings.clock_${choice}`)}</option>
                  {/each}
                </select>
              </Row>
            </Group>
          {:else if section === 'shortcuts'}
            <Group>
              {#each ACTIONS as action (action)}
                <Row label={t(`shortcuts.${action}`)}>
                  <button
                    class="btn key"
                    aria-label={`${t(`shortcuts.${action}`)}, ${displayBinding(shortcutFor(action))}`}
                    onclick={() => (recording = action)}
                  >
                    {recording === action
                      ? t('settings.pressKeys')
                      : displayBinding(shortcutFor(action))}
                  </button>
                </Row>
              {/each}
            </Group>
            <!-- only once something was changed -->
            {#if Object.keys(prefs.shortcuts).length}
              <div class="settings-actions">
                <button class="btn" onclick={() => (prefs.shortcuts = {})}
                  >{t('settings.resetShortcuts')}</button
                >
              </div>
            {/if}
          {:else if section === 'security'}
            {@const vault = settings.vault}
            {#if vault && !vault.encrypted}
              <Group title={t('settings.notEncryptedTitle')}>
                <Row label={t('settings.phone.plain')}>
                  <button class="btn primary" onclick={() => settings.encrypt()}
                    ><Icon name="lock" size={16} />{t('settings.encrypt')}</button
                  >
                </Row>
              </Group>
            {/if}
            {#if vault?.encrypted}
              <div role="radiogroup" aria-label={t('settings.wayTitle')}>
                <Group title={t('settings.wayTitle')}>
                  {#each ways(vault) as option (option)}
                    <Row
                      label={t(`settings.way.${option}`, { method })}
                      help={t(`settings.wayHelp.${option}`, { method })}
                      checked={vault.way === option}
                      onclick={() => void settings.setWay(option)}
                    />
                  {/each}
                  {#if vault.way === 'pin'}
                    <Row label={t('pin.change')}>
                      <button class="btn" onclick={() => settings.setWay('pin')}
                        >{t('pin.change')}</button
                      >
                    </Row>
                  {/if}
                </Group>
              </div>
              {#if vault.way === 'guarded' || vault.way === 'pin' || vault.way === 'password'}
                <Group>
                  <Row label={t('settings.autoLock')}>
                    <select
                      class="field compact"
                      aria-label={t('settings.autoLock')}
                      bind:value={prefs.autoLockMinutes}
                    >
                      {#each LOCK_CHOICES as minutes (minutes)}
                        <option value={minutes}>{lockLabel(minutes)}</option>
                      {/each}
                    </select>
                  </Row>
                  <Row label={t('settings.lockNow')}>
                    <button class="btn" onclick={onlock}
                      ><Icon name="lock" size={16} />{t('sidebar.lock')}</button
                    >
                  </Row>
                </Group>
              {/if}
              <Group
                title={t('settings.phone.groupPassword')}
                note={vault.recovery ? t('recovery.has') : t('recovery.noneNow')}
              >
                {#if settings.recoveryCode}
                  <div class="inside">
                    <RecoveryKey
                      code={settings.recoveryCode}
                      done={() => settings.recoveryShown()}
                      level="h3"
                    />
                  </div>
                {:else}
                  <Row label={t('recovery.heading')}>
                    <span class="state"
                      >{vault.recovery
                        ? t('settings.phone.keyCreated')
                        : t('settings.phone.keyNone')}</span
                    >
                    <button class="btn" onclick={() => settings.createRecovery()}
                      >{vault.recovery ? t('recovery.replace') : t('recovery.create')}</button
                    >
                    {#if vault.recovery}
                      <button class="btn danger" onclick={() => settings.removeRecovery()}
                        >{t('recovery.remove')}</button
                      >
                    {/if}
                  </Row>
                {/if}
                {#if vault.hasPassword}
                  <Row label={t('settings.changePassword')}>
                    <button class="btn" onclick={() => (changingPassword = !changingPassword)}
                      >{changingPassword
                        ? t('common.cancel')
                        : t('settings.changePassword')}</button
                    >
                  </Row>
                  {#if changingPassword}
                    <form class="inside under" onsubmit={changePassword}>
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
                          class="btn primary"
                          type="submit"
                          disabled={!oldPassword || newPassword.length < MIN_PASSWORD}
                          >{t('settings.changePassword')}</button
                        >
                      </div>
                    </form>
                  {/if}
                {/if}
              </Group>
            {/if}
          {:else if section === 'sync'}
            <SyncSettings />
          {:else if section === 'backups'}
            <BackupSettings onbackup={(mode) => (backingUp = mode)} />
          {:else if section === 'data'}
            <Group title={t('settings.importTitle')} note={t('settings.importHelp')}>
              <!-- also in the list's header, here with the other ways in -->
              <Row label={t('list.import')} opens onclick={() => markdownInput?.click()} />
              <Row
                label={t('settings.import')}
                opens
                disabled={!backend.native}
                onclick={() => (importing = 'other')}
              />
              <Row
                label={t('settings.importFicelle', { app: brand.displayName })}
                opens
                disabled={!backend.native}
                onclick={() => (importing = 'ficelle')}
              />
              <!-- backups come back from Backups, where they are found and listed -->
              <Row label={t('backups.restoreFrom')} opens onclick={() => (section = 'backups')} />
            </Group>
            <input
              bind:this={markdownInput}
              type="file"
              accept=".md,.markdown,.mdown,.mkd,text/markdown"
              multiple
              hidden
              onchange={async (e) => {
                const input = e.currentTarget
                await workspace.importFiles([...(input.files ?? [])])
                input.value = ''
              }}
            />
            <Group title={t('settings.exportTitle')} note={t('settings.exportHelp')}>
              <Row
                label={t('settings.export')}
                opens
                disabled={!backend.native}
                onclick={() => settings.exportNotes()}
              />
              <Row
                label={t('settings.exportZip')}
                opens
                disabled={!backend.native}
                onclick={() => settings.exportZip()}
              />
            </Group>
            <Group>
              <Row label={t('reset.title')} help={t('reset.help')}>
                <button class="btn danger" onclick={() => (resetting = true)}
                  >{t('reset.open')}</button
                >
              </Row>
            </Group>
          {:else if section === 'experimental'}
            <p class="settings-help">{t('settings.experimentalHelp')}</p>
            <LayoutPicker />
          {:else if section === 'testTools'}
            <TestTools />
          {:else if section === 'feedback'}
            <FeedbackSettings />
          {:else if section === 'about'}
            <Group>
              <Row label={t('settings.version')}
                ><span class="value mono">{__APP_VERSION__}</span></Row
              >
              <Row label={t('settings.buildDate')}
                ><span class="value mono">{__APP_BUILD_DATE__}</span></Row
              >
              <Row label={t('settings.protocol')}
                ><span class="value mono">{__APP_PROTOCOL__}</span></Row
              >
              {#if serverProtocols}
                <Row label={t('settings.serverProtocol')}
                  ><span class="value mono">{serverProtocols}</span></Row
                >
              {/if}
            </Group>
            {#if updatesHere}
              <Group title={t('appUpdate.title')}>
                <div class="inside"><UpdateCheck /></div>
              </Group>
            {/if}
            <Group>
              <div class="inside"><ContactLine /></div>
              <Row label={t('settings.credits')} onclick={() => (creditsOpen = !creditsOpen)}>
                <span class="opens" class:open={creditsOpen}><Icon name="down" size={16} /></span>
              </Row>
              {#if creditsOpen}<div class="inside"><CreditsList /></div>{/if}
            </Group>
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
    /* over the open note and its toolbar (30) and history (35), under the
       report button, toasts and dialogs (40 and up) */
    z-index: 38;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: color-mix(in srgb, var(--color-bg) 70%, transparent);
  }
  .dialog {
    width: min(820px, 100%);
    /* one size for every section: a percentage of the backdrop resolved to
       nothing and the window followed each section's length */
    height: min(620px, calc(100dvh - 2 * var(--space-4)));
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
  /* one quiet tile for every section, the icon in the accent, as on the
     phone: the colour says "a setting", not which kind */
  .tile {
    display: grid;
    width: 22px;
    height: 22px;
    flex: none;
    place-items: center;
    border-radius: var(--radius-control);
    background: var(--color-surface-2);
    color: var(--color-accent-text);
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
  .failed {
    color: var(--color-danger);
  }
  /* a menu in a settings row, as wide as its longest choice */
  .content :global(.field.compact) {
    width: auto;
    min-width: 160px;
    height: 30px;
  }
  /* what opens inside a group, under its row: a form, the new key */
  .inside {
    display: grid;
    justify-items: start;
    gap: var(--space-2);
    padding: var(--space-3);
  }
  /* opened under its row: the row above gives the space */
  .inside.under {
    padding-top: 0;
  }
  .value {
    color: var(--color-text-muted);
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 13px;
  }
  .opens {
    display: grid;
    color: var(--color-text-faint);
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .opens.open {
    transform: rotate(180deg);
  }
  .state {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .key {
    min-width: 110px;
    justify-content: center;
    font-family: var(--font-mono);
  }
  .message {
    margin: var(--space-5) 0 0;
  }
</style>
