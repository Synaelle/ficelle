<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Settings on phones, grouped lists like a phone's own settings: coloured
     icon tiles, the current value on the right, a short note under groups.
     Every row is one tap from the first screen, details open a page. -->
<script lang="ts">
  import { channelSwitch } from '../lib/channelChange.svelte'
  import { serverProtocol, sync } from '../lib/sync.svelte'
  import SyncSettings from './SyncSettings.svelte'
  import { brand } from '@app/brand'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Switch from './ui/Switch.svelte'
  import { tick, untrack } from 'svelte'
  import { onBack } from '../lib/back'
  import { modal } from '../lib/modal'
  import { biometricMethod } from '../lib/biometric'
  import { LOCALES, t, type Locale } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { overlay, type MenuItem } from '../lib/overlay.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import AccentPicker from './AccentPicker.svelte'
  import CornersPicker from './CornersPicker.svelte'
  import ArrowPicker from './ArrowPicker.svelte'
  import BackupSettings from './BackupSettings.svelte'
  import FeedbackSettings from './FeedbackSettings.svelte'
  import TestTools from '../devreport/TestTools.svelte'
  import ContactLine from './ContactLine.svelte'
  import CreditsList from './CreditsList.svelte'
  import { backend } from '../lib/backend'
  import { ANDROID } from '../lib/platform'
  import { backups } from '../lib/backups.svelte'
  import { UI_SIZES } from '../lib/uiSize'
  import { workspace } from '../lib/workspace.svelte'
  import RecoveryKey from '../screens/RecoveryKey.svelte'
  import UpdateCheck from './UpdateCheck.svelte'
  import FontPicker from './FontPicker.svelte'
  import { channelChoice, updatesHere } from '../lib/appUpdate.svelte'
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
    | 'credits'
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

  interface SettingRow {
    icon: IconName
    label: string
    value?: string
    /** More words the search finds the row by. */
    keywords?: string
    /** A switch instead of a value. */
    on?: boolean
    /** Does its thing at once, no page opens: no arrow. */
    action?: boolean
    danger?: boolean
    /** Shown, but nothing to do here (the browser has no files). */
    disabled?: boolean
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

  const lookRows = $derived<SettingRow[]>([
    {
      icon: 'palette',
      label: t('settings.appearance'),
      keywords: t('settings.appearanceKeywords'),
      value: t(`settings.mode_${prefs.mode}`),
      run: () => (page = 'appearance'),
    },
    {
      icon: 'textSize',
      label: t('settings.accessibility'),
      keywords: t('settings.accessibilityKeywords'),
      value: t(`settings.uiSize_${prefs.uiSize}`),
      run: () => (page = 'accessibility'),
    },
    {
      icon: 'textStyle',
      label: t('settings.editor'),
      keywords: t('settings.editorKeywords'),
      run: () => (page = 'editor'),
    },
    {
      icon: 'language',
      label: t('settings.language'),
      keywords: t('settings.languageKeywords'),
      value: languageName,
      run: () => (page = 'language'),
    },
  ])

  /** Notes out of screenshots and the recent apps, where Android can. */
  const canHide = typeof window.AppSystem?.setHidden === 'function'
  let hidden = $state(window.AppSystem?.hidden?.() ?? false)
  const hideRows = $derived<SettingRow[]>(
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

  const lockRows = $derived<SettingRow[]>(
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
                  {
                    icon: 'lock' as const,
                    label: t('settings.lockNow'),
                    action: true,
                    run: onlock,
                  },
                ]
              : []),
          ],
  )
  const privacyRows = $derived<SettingRow[]>([...lockRows, ...hideRows])

  const passwordRows = $derived<SettingRow[]>(
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

  const syncRows = $derived<SettingRow[]>([
    {
      icon: sync.status.state === 'off' ? 'offline' : 'synced',
      label: t('settings.sync'),
      keywords: t('settings.syncKeywords'),
      value: t(`sync.state.${sync.status.state}`),
      run: () => (page = 'sync'),
    },
  ])

  // the Android app, not a narrow window
  const android = ANDROID
  const noteRows = $derived<SettingRow[]>([
    {
      icon: 'backup',
      label: t('settings.backups'),
      keywords: t('settings.backupsKeywords'),
      value: backups.view?.config ? t(`backups.schedule_${backups.view.config.schedule}`) : '',
      run: () => (page = 'backups'),
    },
  ])

  // the browser has no files to reach: shown, but not tapped
  const importRows = $derived<SettingRow[]>([
    {
      icon: 'archive',
      label: t('settings.importFicelle', { app: brand.displayName }),
      disabled: !backend.native,
      run: () => onimport(true),
    },
    {
      icon: 'import',
      label: t('settings.import'),
      disabled: !backend.native,
      run: () => onimport(false),
    },
    { icon: 'markdown', label: t('list.import'), run: () => fileInput?.click() },
  ])
  const exportRows = $derived<SettingRow[]>([
    {
      icon: 'export',
      // Android's folder picker gives no path to write files under: one zip
      label: android ? t('settings.exportZip') : t('settings.export'),
      keywords: t('settings.dataKeywords'),
      disabled: !backend.native,
      action: true,
      run: () => void (android ? settings.exportZip() : settings.exportNotes()),
    },
    ...(android
      ? []
      : [
          {
            icon: 'archive' as const,
            label: t('settings.exportZip'),
            keywords: t('settings.dataKeywords'),
            disabled: !backend.native,
            action: true,
            run: () => void settings.exportZip(),
          },
        ]),
  ])

  // one row on the first screen opens the import, export and backup rows
  const dataRow = $derived<SettingRow>({
    icon: 'backup',
    label: t('settings.phone.dataBackups'),
    keywords: t('settings.dataKeywords'),
    run: () => (page = 'notes'),
  })

  const aboutRows = $derived<SettingRow[]>([
    {
      icon: 'feedback',
      label: t('settings.feedback'),
      keywords: t('settings.feedbackKeywords'),
      run: () => (page = 'feedback'),
    },
    // dev and preview builds only
    ...(__APP_DEV_TOOLS__
      ? [
          {
            icon: 'log' as const,
            label: t('settings.testTools'),
            keywords: t('settings.testToolsKeywords'),
            run: () => (page = 'testTools'),
          },
        ]
      : []),
    {
      icon: 'info',
      label: t('settings.about'),
      keywords: t('settings.aboutKeywords'),
      value: __APP_VERSION__,
      run: () => (page = 'about'),
    },
  ])

  /** Settings inside the pages, found by their own name: the page they sit
   * on shows as the value, a tap opens it. */
  const inner: [IconName, string, Exclude<Page, 'home'>][] = [
    ['palette', 'settings.pureBlack', 'appearance'],
    ['palette', 'settings.warmLight', 'appearance'],
    ['textStyle', 'fonts.title', 'appearance'],
    ['palette', 'settings.accent', 'appearance'],
    ['palette', 'settings.corners', 'appearance'],
    ['textSize', 'settings.uiSize', 'accessibility'],
    ['textStyle', 'settings.dyslexicFont', 'accessibility'],
    ['list', 'settings.autoLists', 'editor'],
    ['emoji', 'settings.emojiShortcodes', 'editor'],
    ['right', 'settings.arrow', 'editor'],
    ['timer', 'settings.clock', 'language'],
    ['info', 'settings.credits', 'credits'],
  ]
  if (updatesHere) inner.push(['info', 'appUpdate.check', 'about'])
  if (updatesHere && channelChoice) inner.push(['info', 'appUpdate.channel', 'about'])
  const innerRows = $derived<SettingRow[]>(
    inner.map(([icon, label, to]) => ({
      icon,
      label: t(label),
      value: t(titles[to]),
      run: () => (page = to),
    })),
  )

  const matches = $derived.by(() => {
    const q = query.trim().toLowerCase()
    const found = (r: SettingRow) =>
      r.label.toLowerCase().includes(q) || !!r.keywords?.toLowerCase().includes(q)
    // the PC's Security section is the phone's Privacy and Password groups,
    // both found by its words
    const security = [...lockRows, ...passwordRows].map((r) => ({
      ...r,
      keywords: t('settings.securityKeywords'),
    }))
    const rows = [
      ...lookRows,
      ...security,
      ...hideRows,
      ...syncRows,
      dataRow,
      ...noteRows,
      ...importRows,
      ...exportRows,
      ...aboutRows,
      ...innerRows,
    ].filter(found)
    // a row reached two ways shows once
    return rows.filter((r, i) => rows.findIndex((o) => o.label === r.label) === i)
  })

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
    credits: 'settings.credits',
  }

  /** Backups opens from the data page, and goes back to it. */
  const parentOf = (p: Page): Page =>
    p === 'backups' ? 'notes' : p === 'credits' ? 'about' : 'home'
  const parent = $derived(parentOf(page))

  function back() {
    page = parent
    settings.message = ''
  }
  const onPage = $derived(page !== 'home')
  $effect(() => {
    if (onPage) return onBack(back)
  })

  // each page keeps its place: going back finds the list where it was left,
  // a page opened starts at its top
  let screen: HTMLDivElement | undefined = $state()
  const places: Partial<Record<Page, number>> = {}
  // the page as last drawn, apart from `page` which already moved on
  let shown: Page = untrack(() => page)
  $effect.pre(() => {
    const next = page
    if (!screen || next === shown) return
    const box = screen
    places[shown] = box.scrollTop
    const returning = next === parentOf(shown)
    shown = next
    void tick().then(() => (box.scrollTop = returning ? (places[next] ?? 0) : 0))
  })
</script>

{#snippet row(r: SettingRow)}
  <button
    class="row"
    class:danger={r.danger}
    role={r.on === undefined ? undefined : 'switch'}
    aria-checked={r.on}
    disabled={r.disabled}
    onclick={(e) => r.run(e)}
  >
    <span class="tile"><Icon name={r.icon} size={16} /></span>
    <span class="label">{r.label}</span>
    {#if r.on !== undefined}
      <Switch drawn checked={r.on} />
    {:else}
      {#if r.value}<span class="value">{r.value}</span>{/if}
      {#if !r.action}<Icon name="right" size={16} />{/if}
    {/if}
  </button>
{/snippet}

<!-- a few short choices on one line, the whole width of the group -->
{#snippet segments<T>(
  name: string,
  label: string,
  choices: readonly T[],
  current: T,
  text: (choice: T) => string,
  pick: (choice: T) => void,
)}
  <div class="settings-row segments-row">
    <div class="segmented segments" role="radiogroup" aria-label={label}>
      {#each choices as choice, i (i)}
        <label class:checked={current === choice}>
          <input type="radio" {name} checked={current === choice} onchange={() => pick(choice)} />
          {text(choice)}
        </label>
      {/each}
    </div>
  </div>
{/snippet}

<div
  bind:this={screen}
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

    <div class="groups">
      {#if query.trim()}
        {#if matches.length === 0}
          <p class="empty">{t('list.noResults')}</p>
        {:else}
          <Group>
            {#each matches as r (r.label)}{@render row(r)}{/each}
          </Group>
        {/if}
      {:else}
        <Group title={t('settings.phone.groupLook')}>
          {#each lookRows as r (r.label)}{@render row(r)}{/each}
        </Group>
        {#if privacyRows.length}
          <Group
            title={vault && !encrypted
              ? t('settings.notEncryptedTitle')
              : t('settings.phone.groupPrivacy')}
          >
            {#each privacyRows as r (r.label)}{@render row(r)}{/each}
          </Group>
          <!-- an unencrypted vault has no password, recovery key or way to unlock -->
          {#if passwordRows.length}
            <Group
              title={t('settings.phone.groupPassword')}
              note={vault?.recovery ? t('recovery.has') : t('recovery.noneNow')}
            >
              {#each passwordRows as r (r.label)}{@render row(r)}{/each}
            </Group>
          {/if}
        {/if}
        <Group>
          {#each syncRows as r (r.label)}{@render row(r)}{/each}
          {@render row(dataRow)}
        </Group>
        <Group>
          {#each aboutRows as r (r.label)}{@render row(r)}{/each}
        </Group>
      {/if}
    </div>
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

    <div class="groups page">
      {#if page === 'appearance'}
        <Group title={t('settings.mode')}>
          {@render segments(
            'mode',
            t('settings.mode'),
            ['system', 'dark', 'light'] as const,
            prefs.mode,
            (mode) => t(`settings.mode_${mode}`),
            (mode) => (prefs.mode = mode),
          )}
          <Row
            label={t('settings.pureBlack')}
            on={prefs.pureBlack}
            onclick={() => (prefs.pureBlack = !prefs.pureBlack)}
          />
          <Row
            label={t('settings.warmLight')}
            on={prefs.warmLight}
            onclick={() => (prefs.warmLight = !prefs.warmLight)}
          />
        </Group>
        <Group title={t('settings.accent')}><AccentPicker row /></Group>
        <Group title={t('fonts.title')}>
          <FontPicker role="notes" row />
          <FontPicker role="code" row />
        </Group>
        <Group title={t('settings.corners')} note={t('settings.cornersHelp')}>
          <div class="inside"><CornersPicker /></div>
        </Group>
      {:else if page === 'accessibility'}
        <Group title={t('settings.uiSize')} note={t('settings.uiSizeHelp')}>
          {@render segments(
            'size',
            t('settings.uiSize'),
            UI_SIZES,
            prefs.uiSize,
            (size) => t(`settings.uiSize_${size}`),
            (size) => (prefs.uiSize = size),
          )}
        </Group>
        <Group>
          <Row
            label={t('settings.dyslexicFont')}
            on={prefs.dyslexicFont}
            onclick={() => (prefs.dyslexicFont = !prefs.dyslexicFont)}
          />
          {#if prefs.dyslexicFont && dyslexicStatus.loading}
            <p class="under" role="status">{t('settings.dyslexicFontLoading')}</p>
          {:else if prefs.dyslexicFont && dyslexicStatus.failed}
            <p class="under error" role="alert">{t('settings.dyslexicFontFailed')}</p>
          {/if}
        </Group>
      {:else if page === 'editor'}
        <Group>
          <Row
            label={t('settings.autoLists')}
            on={prefs.autoLists}
            onclick={() => (prefs.autoLists = !prefs.autoLists)}
          />
          <Row
            label={t('settings.emojiShortcodes')}
            on={prefs.emojiShortcodes}
            onclick={() => (prefs.emojiShortcodes = !prefs.emojiShortcodes)}
          />
        </Group>
        <Group title={t('settings.arrow')} typed>
          <div class="inside"><ArrowPicker /></div>
        </Group>
      {:else if page === 'language'}
        <Group title={t('settings.language')}>
          <Row
            label={t('settings.languageSystem')}
            checked={prefs.language === null}
            onclick={() => (prefs.language = null)}
          />
          {#each LOCALES as locale (locale.code)}
            <Row
              label={locale.name}
              checked={prefs.language === locale.code}
              onclick={() => (prefs.language = locale.code as Locale)}
            />
          {/each}
        </Group>
        <Group title={t('settings.clock')}>
          {#each ['system', '24', '12'] as const as choice (choice)}
            <Row
              label={t(`settings.clock_${choice}`)}
              checked={prefs.clock === choice}
              onclick={() => (prefs.clock = choice)}
            />
          {/each}
        </Group>
      {:else if page === 'lock'}
        <Group>
          {#each LOCK_CHOICES as minutes (minutes)}
            <Row
              label={lockLabel(minutes)}
              checked={prefs.autoLockMinutes === minutes}
              onclick={() => (prefs.autoLockMinutes = minutes)}
            />
          {/each}
        </Group>
      {:else if page === 'way' && vault}
        <Group>
          {#each ways(vault) as option (option)}
            <Row
              label={t(`settings.way.${option}`, { method })}
              help={t(`settings.wayHelp.${option}`, { method })}
              checked={vault.way === option}
              onclick={() => void settings.setWay(option)}
            />
          {/each}
          {#if vault.way === 'pin'}
            <Row label={t('pin.change')} opens onclick={() => settings.setWay('pin')} />
          {/if}
        </Group>
      {:else if page === 'sync'}
        <SyncSettings />
      {:else if page === 'notes'}
        <Group>
          {#each noteRows as r (r.label)}{@render row(r)}{/each}
        </Group>
        <Group title={t('settings.importTitle')} note={t('settings.importHelp')}>
          {#each importRows as r (r.label)}{@render row(r)}{/each}
        </Group>
        <Group title={t('settings.exportTitle')} note={t('settings.exportHelp')}>
          {#each exportRows as r (r.label)}{@render row(r)}{/each}
        </Group>
        <!-- with the data, as on desktop -->
        <Group note={t('reset.help')}>
          {@render row({
            icon: 'restore',
            label: t('reset.title'),
            danger: true,
            run: onreset,
          })}
        </Group>
      {:else if page === 'backups'}
        <BackupSettings {onbackup} />
      {:else if page === 'feedback'}
        <FeedbackSettings />
      {:else if page === 'testTools'}
        <TestTools />
      {:else if page === 'about'}
        <Group>
          <Row label={t('settings.version')}><span class="value mono">{__APP_VERSION__}</span></Row>
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
          {#if channelChoice}
            <Group title={t('appUpdate.channel')}>
              {#each ['stable', 'beta'] as const as choice (choice)}
                <Row
                  label={choice === 'stable'
                    ? t('appUpdate.channelStable')
                    : t('appUpdate.channelBeta')}
                  help={choice === 'stable'
                    ? t('appUpdate.channelStableHelp')
                    : t('appUpdate.channelBetaHelp')}
                  checked={prefs.channel === choice}
                  onclick={() => void channelSwitch.choose(choice)}
                />
              {/each}
            </Group>
          {/if}
          <UpdateCheck rows />
        {/if}
        <Group>
          <ContactLine row />
          <Row label={t('settings.credits')} opens onclick={() => (page = 'credits')} />
        </Group>
      {:else if page === 'credits'}
        <CreditsList phone />
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
    /* over the open note and its toolbar (30) and history (35), under the
       report button, toasts and dialogs (40 and up) */
    z-index: 38;
    overflow-y: auto;
    padding: 0 var(--space-4) var(--space-6);
    background: var(--color-bg);
  }
  .large {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) 0 var(--space-2) var(--space-1);
  }
  h1 {
    margin: 0;
    font-size: 28px;
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
  /* every page the same: groups one under the other, the same gap between */
  .groups {
    display: grid;
    align-content: start;
    gap: var(--space-5);
    min-width: 0;
    margin-top: var(--space-4);
  }
  .groups.page {
    margin-top: var(--space-1);
  }
  .groups > :global(*) {
    min-width: 0;
  }
  .page {
    padding-top: var(--space-1);
  }
  .empty {
    margin: 0 var(--space-4);
    color: var(--color-text-muted);
    font-size: 13px;
  }

  /* the settings groups (settings/ui/), shared with the desktop, on a phone:
     flat on the page, the captions small capitals, rows a finger's height */
  .screen :global(section.group > .panel),
  .screen :global(.settings-panel) {
    border: 0;
    border-radius: 14px;
    background: var(--color-surface);
  }
  .screen :global(section.group > h3) {
    margin: 0 var(--space-4);
    color: var(--color-text-muted);
    font-size: 13px;
    font-weight: 400;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .screen :global(section.group > .note) {
    margin: 0 var(--space-4);
    font-size: 13px;
  }
  /* a page's opening line lines up with the captions */
  .groups > :global(.settings-help),
  .screen :global(.sync-form > .settings-help) {
    margin: 0 var(--space-4);
  }
  .screen :global(.settings-row) {
    min-height: 48px;
    font-size: 16px;
  }
  .screen :global(.settings-row .help) {
    font-size: 13px;
  }
  .screen :global(.field.compact) {
    width: 170px;
    min-height: 36px;
    height: 36px;
    padding-block: 0;
    font-size: 14px;
  }

  .segments-row {
    position: relative;
    padding: var(--space-2) var(--space-3);
  }
  .segments {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
  }
  .segments label {
    display: grid;
    place-items: center;
    min-height: 40px;
    padding: 0 var(--space-1);
    text-align: center;
  }
  .inside {
    padding: var(--space-3);
  }
  .under {
    margin: 0;
    padding: 0 var(--space-3) var(--space-2);
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .under.error {
    color: var(--color-danger);
  }

  .row {
    position: relative;
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 48px;
    padding: 0 var(--space-3);
    border: 0;
    background: none;
    color: var(--color-text);
    font: inherit;
    font-size: 16px;
    text-align: left;
  }
  .row:active:not(:disabled) {
    background: var(--color-surface-2);
  }
  .row:disabled {
    opacity: 0.5;
    cursor: default;
  }
  /* hairlines between rows start after the icon, like a phone's own lists */
  .row + .row::before {
    content: '';
    position: absolute;
    top: 0;
    left: calc(var(--space-3) * 2 + 28px);
    right: 0;
    border-top: 1px solid var(--color-border);
  }
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
    width: 28px;
    height: 28px;
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
  /* a value in a shared row (About): nothing else beside it to give way to */
  :global(.settings-row) .value {
    max-width: none;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 13px;
  }
  .bar {
    position: sticky;
    top: 0;
    z-index: 1;
    display: grid;
    /* the back label keeps its whole text: with a long title or a wide font
       the title gives way, centered for as long as it fits */
    grid-template-columns: minmax(max-content, 1fr) minmax(0, auto) minmax(0, 1fr);
    column-gap: var(--space-2);
    align-items: center;
    min-height: 52px;
    margin: 0 calc(var(--space-4) * -1);
    padding: 0 var(--space-2);
    background: var(--color-bg);
  }
  .bar b {
    font-size: 17px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  .message {
    margin: var(--space-4) var(--space-4) 0;
    color: var(--color-text-muted);
  }
</style>
