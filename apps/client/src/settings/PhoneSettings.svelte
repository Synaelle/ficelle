<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Settings on phones, grouped lists like a phone's own settings: coloured
     icon tiles, the current value on the right, a short note under groups.
     Every row is one tap from the first screen, details open a page. -->
<script lang="ts">
  import { serverProtocol, sync } from '../lib/sync.svelte'
  import SyncSettings from './SyncSettings.svelte'
  import { brand } from '@app/brand'
  import { onBack } from '../lib/back'
  import { modal } from '../lib/modal'
  import { biometricMethod } from '../lib/biometric'
  import { LOCALES, t, type Locale } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { overlay, type MenuItem } from '../lib/overlay.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import AccentPicker from './AccentPicker.svelte'
  import BackupSettings from './BackupSettings.svelte'
  import FeedbackSettings from './FeedbackSettings.svelte'
  import TestTools from '../devreport/TestTools.svelte'
  import { backups } from '../lib/backups.svelte'
  import { UI_SIZES } from '../lib/uiSize'
  import { workspace } from '../lib/workspace.svelte'
  import RecoveryKey from '../screens/RecoveryKey.svelte'
  import logo from '../../../../packages/ui/brand/app-icon.svg?url'
  import UpdateCheck from './UpdateCheck.svelte'
  import FontPicker from './FontPicker.svelte'
  import { updatesHere } from '../lib/appUpdate.svelte'
  import { dyslexicStatus } from '../lib/dyslexicStatus.svelte'
  import { LOCK_CHOICES, lockLabel, ways, type SettingsState } from './settingsState.svelte'

  let {
    settings,
    close,
    onlock,
    onimport,
    onbackup,
    onreset,
    start,
  }: {
    settings: SettingsState
    close: () => void
    onlock: () => void
    /** Opens the import from another app. */
    /** `true` for this app's own export. */
    onimport: (ficelle: boolean) => void
    /** Opens making or restoring a backup. */
    onbackup: (mode: 'make' | 'restore') => void
    /** Opens deleting everything on this device. */
    onreset: () => void
    start?: 'sync'
  } = $props()

  type Page =
    | 'home'
    | 'appearance'
    | 'accessibility'
    | 'editor'
    | 'language'
    | 'lock'
    | 'way'
    | 'sync'
    | 'notes'
    | 'backups'
    | 'feedback'
    | 'testTools'
    | 'about'
  // svelte-ignore state_referenced_locally
  let page: Page = $state(start ?? 'home')
  /** The linked server's protocol versions, asked when About opens. */
  let serverProtocols: string | null = $state(null)
  $effect(() => {
    if (page === 'about') void serverProtocol().then((v) => (serverProtocols = v))
  })
  let query = $state('')
  let fileInput: HTMLInputElement | undefined = $state()

  const method = $derived(biometricMethod())
  const vault = $derived(settings.vault)
  const encrypted = $derived(vault?.encrypted === true)
  /** Only a vault that asks for something to open has anything to lock. */
  const lockable = $derived(
    vault?.way === 'guarded' || vault?.way === 'pin' || vault?.way === 'password',
  )

  interface Row {
    icon: IconName
    label: string
    value?: string
    /** A switch instead of a value. */
    on?: boolean
    danger?: boolean
    run: (event: MouseEvent) => void
  }

  const languageName = $derived(
    LOCALES.find((l) => l.code === prefs.language)?.name ?? t('settings.languageSystem'),
  )

  function recoveryMenu(event: MouseEvent) {
    if (!vault?.recovery) return void settings.createRecovery()
    const items: MenuItem[] = [
      { label: t('recovery.replace'), icon: 'key', run: () => void settings.createRecovery() },
      {
        label: t('recovery.remove'),
        icon: 'trash',
        danger: true,
        run: () => void settings.removeRecovery(),
      },
    ]
    overlay.openMenuBelow(event.currentTarget as HTMLElement, items)
  }

  async function importPicked(input: HTMLInputElement) {
    await workspace.importFiles([...(input.files ?? [])])
    input.value = ''
  }

  const lookRows = $derived<Row[]>([
    {
      icon: 'palette',
      label: t('settings.appearance'),
      value: t(`settings.mode_${prefs.mode}`),
      run: () => (page = 'appearance'),
    },
    {
      icon: 'textSize',
      label: t('settings.accessibility'),
      value: t(`settings.uiSize_${prefs.uiSize}`),
      run: () => (page = 'accessibility'),
    },
    {
      icon: 'textStyle',
      label: t('settings.editor'),
      value: prefs.autoLists ? t('settings.phone.listsOn') : t('settings.phone.listsOff'),
      run: () => (page = 'editor'),
    },
    {
      icon: 'language',
      label: t('settings.language'),
      value: languageName,
      run: () => (page = 'language'),
    },
  ])

  /** Notes out of screenshots and the recent apps, where Android can. */
  const canHide = typeof window.AppSystem?.setHidden === 'function'
  let hidden = $state(window.AppSystem?.hidden?.() ?? false)
  const hideRows = $derived<Row[]>(
    canHide
      ? [
          {
            icon: 'eyeOff',
            label: t('settings.phone.hideScreens'),
            on: hidden,
            run: () => {
              hidden = !hidden
              window.AppSystem?.setHidden?.(hidden)
            },
          },
        ]
      : [],
  )

  const lockRows = $derived<Row[]>(
    !vault
      ? []
      : !encrypted
        ? [{ icon: 'lock', label: t('settings.encrypt'), run: () => void settings.encrypt() }]
        : [
            {
              icon: 'fingerprint',
              label: t('settings.wayTitle'),
              value: t(`settings.way.${vault.way}`, { method }),
              run: () => (page = 'way'),
            },
            ...(lockable
              ? [
                  {
                    icon: 'timer' as const,
                    label: t('settings.autoLock'),
                    value: lockLabel(prefs.autoLockMinutes),
                    run: () => (page = 'lock'),
                  },
                  { icon: 'lock' as const, label: t('settings.phone.lockNow'), run: onlock },
                ]
              : []),
          ],
  )
  const privacyRows = $derived<Row[]>([...lockRows, ...hideRows])

  const passwordRows = $derived<Row[]>(
    encrypted
      ? [
          {
            icon: 'key',
            label: t('recovery.heading'),
            value: vault?.recovery ? t('settings.phone.keyCreated') : t('settings.phone.keyNone'),
            run: recoveryMenu,
          },
          ...(vault?.hasPassword
            ? [
                {
                  icon: 'pencil' as const,
                  label: t('settings.changePassword'),
                  run: () => void settings.changePasswordAsked(),
                },
              ]
            : []),
        ]
      : [],
  )

  const syncRows = $derived<Row[]>([
    {
      icon: sync.status.state === 'off' ? 'offline' : 'synced',
      label: t('settings.sync'),
      value: t(`sync.state.${sync.status.state}`),
      run: () => (page = 'sync'),
    },
  ])

  const android = /android/i.test(navigator.userAgent)
  const noteRows = $derived<Row[]>([
    {
      icon: 'backup',
      label: t('settings.backups'),
      value: backups.view?.config ? t(`backups.schedule_${backups.view.config.schedule}`) : '',
      run: () => (page = 'backups'),
    },
    {
      icon: 'import',
      label: t('settings.importFicelle', { app: brand.displayName }),
      run: () => onimport(true),
    },
    { icon: 'import', label: t('settings.import'), run: () => onimport(false) },
    { icon: 'import', label: t('list.import'), run: () => fileInput?.click() },
    {
      icon: 'export',
      // Android's folder picker gives no path to write files under: one zip
      label: android ? t('settings.exportZip') : t('settings.export'),
      run: () => void (android ? settings.exportZip() : settings.exportNotes()),
    },
  ])

  // one row on the first screen opens the import, export and backup rows
  const dataRow = $derived<Row>({
    icon: 'backup',
    label: t('settings.phone.dataBackups'),
    run: () => (page = 'notes'),
  })

  const aboutRows = $derived<Row[]>([
    {
      icon: 'feedback',
      label: t('settings.feedback'),
      run: () => (page = 'feedback'),
    },
    // dev and preview builds only
    ...(__APP_DEV_REPORTS__
      ? [{ icon: 'log' as const, label: t('settings.testTools'), run: () => (page = 'testTools') }]
      : []),
    {
      icon: 'info',
      label: t('settings.about'),
      value: __APP_VERSION__,
      run: () => (page = 'about'),
    },
  ])

  const matches = $derived(
    [...lookRows, ...privacyRows, ...passwordRows, ...syncRows, ...noteRows, ...aboutRows].filter(
      (r) => r.label.toLowerCase().includes(query.trim().toLowerCase()),
    ),
  )

  const titles: Record<Exclude<Page, 'home'>, string> = {
    appearance: 'settings.appearance',
    accessibility: 'settings.accessibility',
    editor: 'settings.editor',
    language: 'settings.language',
    lock: 'settings.autoLock',
    way: 'settings.wayTitle',
    sync: 'settings.sync',
    notes: 'settings.phone.dataBackups',
    backups: 'settings.backups',
    feedback: 'settings.feedback',
    testTools: 'settings.testTools',
    about: 'settings.about',
  }

  /** Backups opens from the data page, and goes back to it. */
  const parentOf = (p: Page): Page => (p === 'backups' ? 'notes' : 'home')
  const parent = $derived(parentOf(page))

  function back() {
    page = parent
    settings.message = ''
  }
  const onPage = $derived(page !== 'home')
  $effect(() => {
    if (onPage) return onBack(back)
  })
</script>

{#snippet row(r: Row)}
  <button
    class="row"
    class:danger={r.danger}
    role={r.on === undefined ? undefined : 'switch'}
    aria-checked={r.on}
    onclick={(e) => r.run(e)}
  >
    <span class="tile"><Icon name={r.icon} size={17} /></span>
    <span class="label">{r.label}</span>
    {#if r.on !== undefined}
      <span class="switch" class:on={r.on}><i></i></span>
    {:else}
      {#if r.value}<span class="value">{r.value}</span>{/if}
      <Icon name="right" size={16} />
    {/if}
  </button>
{/snippet}

{#snippet check(on: boolean)}
  {#if on}<span class="checked"><Icon name="check" size={18} /></span>{/if}
{/snippet}

<div
  class="screen"
  role="dialog"
  aria-modal="true"
  aria-label={t('settings.title')}
  use:modal={{ onclose: close }}
>
  {#if settings.recoveryCode}
    <div class="page">
      <RecoveryKey code={settings.recoveryCode} done={() => settings.recoveryShown()} level="h1" />
    </div>
  {:else if page === 'home'}
    <header class="large">
      <h1>{t('settings.title')}</h1>
      <button class="icon-btn" aria-label={t('common.close')} onclick={close}
        ><Icon name="close" /></button
      >
    </header>
    <label class="search">
      <Icon name="search" size={16} />
      <input type="search" placeholder={t('settings.search')} bind:value={query} />
    </label>

    {#if query.trim()}
      <div class="group">
        {#each matches as r (r.label)}{@render row(r)}{/each}
      </div>
      {#if matches.length === 0}<p class="note">{t('list.noResults')}</p>{/if}
    {:else}
      <div class="hero">
        <img src={logo} alt="" />
        <span>
          <b>{brand.displayName}</b>
          <span>{encrypted ? t('settings.phone.encrypted') : t('settings.phone.plain')}</span>
        </span>
      </div>
      <p class="caption">{t('settings.phone.groupLook')}</p>
      <div class="group">
        {#each lookRows as r (r.label)}{@render row(r)}{/each}
      </div>
      {#if privacyRows.length}
        <p class="caption">{t('settings.phone.groupPrivacy')}</p>
        <div class="group">
          {#each privacyRows as r (r.label)}{@render row(r)}{/each}
        </div>
        <!-- an unencrypted vault has no password, recovery key or way to unlock -->
        {#if passwordRows.length}
          <p class="caption">{t('settings.phone.groupPassword')}</p>
          <div class="group">
            {#each passwordRows as r (r.label)}{@render row(r)}{/each}
          </div>
          <p class="note">{vault?.recovery ? t('recovery.has') : t('recovery.none')}</p>
        {/if}
      {/if}
      <p class="caption">{t('settings.sync')}</p>
      <div class="group">
        {#each syncRows as r (r.label)}{@render row(r)}{/each}
      </div>
      <div class="group spaced">
        {@render row(dataRow)}
      </div>
      <div class="group spaced">
        {#each aboutRows as r (r.label)}{@render row(r)}{/each}
      </div>
    {/if}
  {:else}
    <header class="bar">
      <button class="back" onclick={back}
        ><Icon name="back" size={20} /><span
          >{parent === 'home' ? t('settings.title') : t(titles[parent])}</span
        ></button
      >
      <b>{t(titles[page])}</b>
      <span></span>
    </header>

    <div class="page">
      {#if page === 'appearance'}
        <p class="caption">{t('settings.mode')}</p>
        <div class="group">
          {#each ['system', 'dark', 'light'] as const as mode (mode)}
            <button class="row" onclick={() => (prefs.mode = mode)}>
              <span class="label">{t(`settings.mode_${mode}`)}</span>
              {@render check(prefs.mode === mode)}
            </button>
          {/each}
        </div>
        <div class="group">
          {@render row({
            icon: 'palette',
            label: t('settings.pureBlack'),
            on: prefs.pureBlack,
            run: () => (prefs.pureBlack = !prefs.pureBlack),
          })}
        </div>
        <p class="caption">{t('fonts.title')}</p>
        <div class="group pad fonts">
          <FontPicker role="notes" />
          <FontPicker role="code" />
        </div>
        <p class="caption">{t('settings.accent')}</p>
        <div class="group pad accent-group"><AccentPicker /></div>
      {:else if page === 'accessibility'}
        <p class="caption">{t('settings.uiSize')}</p>
        <div class="group">
          {#each UI_SIZES as size (size)}
            <button class="row" onclick={() => (prefs.uiSize = size)}>
              <span class="label">{t(`settings.uiSize_${size}`)}</span>
              {@render check(prefs.uiSize === size)}
            </button>
          {/each}
        </div>
        <div class="group">
          {@render row({
            icon: 'textStyle',
            label: t('settings.dyslexicFont'),
            on: prefs.dyslexicFont,
            run: () => (prefs.dyslexicFont = !prefs.dyslexicFont),
          })}
        </div>
        {#if prefs.dyslexicFont && dyslexicStatus.loading}
          <p class="note" role="status">{t('settings.dyslexicFontLoading')}</p>
        {:else if prefs.dyslexicFont && dyslexicStatus.failed}
          <p class="note error" role="alert">{t('settings.dyslexicFontFailed')}</p>
        {/if}
      {:else if page === 'editor'}
        <div class="group">
          {@render row({
            icon: 'list',
            label: t('settings.autoLists'),
            on: prefs.autoLists,
            run: () => (prefs.autoLists = !prefs.autoLists),
          })}
          {@render row({
            icon: 'list',
            label: t('settings.emojiShortcodes'),
            on: prefs.emojiShortcodes,
            run: () => (prefs.emojiShortcodes = !prefs.emojiShortcodes),
          })}
          {@render row({
            icon: 'pin',
            label: t('settings.openNoteFirst'),
            on: prefs.openNoteFirst,
            run: () => (prefs.openNoteFirst = !prefs.openNoteFirst),
          })}
        </div>
      {:else if page === 'language'}
        <div class="group">
          <button class="row" onclick={() => (prefs.language = null)}>
            <span class="label">{t('settings.languageSystem')}</span>
            {@render check(prefs.language === null)}
          </button>
          {#each LOCALES as locale (locale.code)}
            <button class="row" onclick={() => (prefs.language = locale.code as Locale)}>
              <span class="label">{locale.name}</span>
              {@render check(prefs.language === locale.code)}
            </button>
          {/each}
        </div>
        <p class="note">{t('settings.clock')}</p>
        <div class="group">
          {#each ['system', '24', '12'] as const as choice (choice)}
            <button class="row" onclick={() => (prefs.clock = choice)}>
              <span class="label">{t(`settings.clock_${choice}`)}</span>
              {@render check(prefs.clock === choice)}
            </button>
          {/each}
        </div>
      {:else if page === 'lock'}
        <div class="group">
          {#each LOCK_CHOICES as minutes (minutes)}
            <button class="row" onclick={() => (prefs.autoLockMinutes = minutes)}>
              <span class="label">{lockLabel(minutes)}</span>
              {@render check(prefs.autoLockMinutes === minutes)}
            </button>
          {/each}
        </div>
      {:else if page === 'way' && vault}
        <div class="group">
          {#each ways(vault) as option (option)}
            <button class="row tall" onclick={() => void settings.setWay(option)}>
              <span class="label">
                {t(`settings.way.${option}`, { method })}
                <span class="sub">{t(`settings.wayHelp.${option}`, { method })}</span>
              </span>
              {@render check(vault.way === option)}
            </button>
          {/each}
        </div>
        {#if vault.way === 'pin'}
          <div class="group spaced">
            {@render row({
              icon: 'key',
              label: t('pin.change'),
              run: () => settings.setWay('pin'),
            })}
          </div>
        {/if}
        {#if settings.message}<p class="note">{settings.message}</p>{/if}
      {:else if page === 'sync'}
        <div class="sync"><SyncSettings /></div>
      {:else if page === 'notes'}
        <div class="group">
          {#each noteRows as r (r.label)}{@render row(r)}{/each}
        </div>
      {:else if page === 'backups'}
        <div class="settings-body padded">
          <BackupSettings {onbackup} />
        </div>
      {:else if page === 'feedback'}
        <div class="settings-body padded">
          <FeedbackSettings />
        </div>
      {:else if page === 'testTools'}
        <div class="settings-body padded">
          <TestTools />
        </div>
      {:else if page === 'about'}
        <div class="group">
          <div class="row static">
            <span class="label">{t('settings.version')}</span>
            <span class="value mono">{__APP_VERSION__}</span>
          </div>
          <div class="row static">
            <span class="label">{t('settings.buildDate')}</span>
            <span class="value mono">{__APP_BUILD_DATE__}</span>
          </div>
          <div class="row static">
            <span class="label">{t('settings.protocol')}</span>
            <span class="value mono">{__APP_PROTOCOL__}</span>
          </div>
          {#if serverProtocols}
            <div class="row static">
              <span class="label">{t('settings.serverProtocol')}</span>
              <span class="value mono">{serverProtocols}</span>
            </div>
          {/if}
        </div>
        {#if updatesHere}
          <div class="group spaced pad">
            <UpdateCheck />
          </div>
        {/if}
        <div class="group spaced">
          {@render row({
            icon: 'restore',
            label: t('reset.title'),
            danger: true,
            run: onreset,
          })}
        </div>
        <p class="note">{t('reset.help')}</p>
      {/if}
    </div>
  {/if}

  {#if settings.message}<p class="message" role="status">{settings.message}</p>{/if}
  <input
    bind:this={fileInput}
    type="file"
    accept=".md,.markdown,.mdown,.mkd,text/markdown"
    multiple
    hidden
    onchange={(e) => importPicked(e.currentTarget)}
  />
</div>

<style>
  .screen {
    position: fixed;
    inset: 0;
    z-index: 20;
    overflow-y: auto;
    padding: 0 var(--space-4) var(--space-6);
    background: var(--color-bg);
  }
  .large {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) 0 var(--space-3) var(--space-1);
  }
  h1 {
    margin: 0;
    font-size: 32px;
    font-weight: 700;
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 44px;
    padding: 0 var(--space-3);
    border-radius: 12px;
    background: var(--color-surface-2);
    color: var(--color-text-muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--color-text);
  }
  .hero {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border-radius: 16px;
    background: var(--color-surface);
  }
  .hero img {
    width: 48px;
    height: 48px;
    border-radius: 12px;
  }
  .hero > span {
    display: grid;
  }
  .hero b {
    font-size: 18px;
  }
  .hero span span {
    color: var(--color-text-muted);
    font-size: 14px;
  }
  .caption {
    margin: var(--space-5) var(--space-4) var(--space-2);
    color: var(--color-text-muted);
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .group {
    overflow: hidden;
    border-radius: 14px;
    background: var(--color-surface);
  }
  .group.spaced,
  .search + .group {
    margin-top: var(--space-5);
  }
  .note {
    margin: var(--space-2) var(--space-4) 0;
    color: var(--color-text-muted);
    font-size: 13px;
  }
  /* a group right after another keeps the gap of a new section */
  .group.pad {
    padding: var(--space-3);
  }
  .group.fonts {
    display: grid;
    gap: var(--space-3);
  }
  /* a group after another, or after a note under one, starts a new section */
  .group + .group,
  .note + .group {
    margin-top: var(--space-5);
  }
  .row {
    position: relative;
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 52px;
    padding: 0 var(--space-3) 0 var(--space-3);
    border: 0;
    background: none;
    color: var(--color-text);
    font: inherit;
    font-size: 16px;
    text-align: left;
  }
  .row:active:not(.static) {
    background: var(--color-surface-2);
  }
  /* hairlines between rows start after the icon, like a phone's own lists */
  .row + .row::before,
  .row.danger .label {
    color: var(--color-danger);
  }
  .row :global(.icon) {
    flex: none;
  }
  .row > :global(.icon:last-child) {
    color: var(--color-text-faint);
  }
  .tile {
    width: 30px;
    height: 30px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 8px;
    /* one quiet tile for every row, the icon in the accent: the colour
       says "a setting", not which kind */
    background: var(--color-surface-2);
    color: var(--color-accent);
  }
  .tile :global(.icon) {
    color: inherit;
  }
  .label {
    flex: 1;
    min-width: 0;
  }
  .value {
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-text-muted);
    font-size: 15px;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 13px;
  }
  .checked {
    color: var(--color-accent-text);
  }
  .switch {
    position: relative;
    width: 50px;
    height: 30px;
    flex: none;
    border-radius: 15px;
    background: var(--color-border-strong);
    transition: background var(--dur-fast) var(--ease-out);
  }
  .switch i {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--color-text);
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .switch.on {
    background: var(--color-accent);
  }
  .switch.on i {
    transform: translateX(20px);
    background: var(--color-accent-on);
  }
  .padded {
    padding: var(--space-2) var(--space-4) 0;
  }
  .note.error {
    color: var(--color-danger);
  }
  .accent-group {
    display: grid;
    gap: var(--space-3);
  }
  .bar {
    position: sticky;
    top: 0;
    z-index: 1;
    display: grid;
    /* the sides may shrink below their text, so a long title or a wide font
       cuts the back label short instead of running into it */
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    column-gap: var(--space-2);
    align-items: center;
    min-height: 52px;
    margin: 0 calc(var(--space-4) * -1);
    padding: 0 var(--space-2);
    background: var(--color-bg);
  }
  .bar b {
    font-size: 17px;
  }
  .back {
    justify-self: start;
    max-width: 100%;
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 44px;
    padding: 0 var(--space-2) 0 0;
    border: 0;
    background: none;
    color: var(--color-accent-text);
    font: inherit;
    font-size: 16px;
  }
  .back span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sync {
    padding: 0 var(--space-4);
  }
  .page {
    padding-top: var(--space-2);
  }
  .page > .group:first-child {
    margin-top: var(--space-3);
  }
  .message {
    margin: var(--space-4) var(--space-4) 0;
    color: var(--color-text-muted);
  }
  .row.tall {
    align-items: center;
    padding-top: var(--space-2);
    padding-bottom: var(--space-2);
  }
  .sub {
    display: block;
    margin-top: 2px;
    font-size: 13px;
    color: var(--color-text-muted);
    white-space: normal;
  }
</style>
