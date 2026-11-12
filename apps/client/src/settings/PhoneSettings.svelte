<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Settings on phones, grouped lists like a phone's own settings: coloured
     icon tiles, the current value on the right, a short note under groups.
     Every row is one tap from the first screen, details open a page. -->
<script lang="ts">
  import { localSync } from '../lib/localSync.svelte'
  import { sync } from '../lib/sync.svelte'
  import SyncSettings from './SyncSettings.svelte'
  import { brand } from '@app/brand'
  import Knot from '@app/ui/Knot.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Switch from './ui/Switch.svelte'
  import { tick, untrack } from 'svelte'
  import { onBack } from '../lib/back'
  import { modal } from '../lib/modal'
  import { biometricMethod } from '../lib/biometric'
  import { LOCALES, LOCALES_URL, t, type Locale } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { overlay, type MenuItem } from '../lib/overlay.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import ArrowPicker from './ArrowPicker.svelte'
  import DataSettings from './DataSettings.svelte'
  import FeedbackSettings from './FeedbackSettings.svelte'
  import TestTools from '../devreport/TestTools.svelte'
  import AboutSettings from './AboutSettings.svelte'
  import { MODES } from '../lib/theme.svelte'
  import { TAG_MARKS } from '../lib/tagMark.svelte'
  import { UI_SIZES } from '../lib/uiSize'
  import RecoveryKey from '../screens/RecoveryKey.svelte'
  import { channelChoice, updatesHere } from '../lib/appUpdate.svelte'
  import { dyslexicStatus } from '../lib/dyslexicStatus.svelte'
  import { LOCK_CHOICES, lockLabel, ways, type SettingsState } from './settingsState.svelte'

  let {
    settings,
    close,
    onlock,
    onimport,
    onrestorefile,
    onreset,
    start,
  }: {
    settings: SettingsState
    close: () => void
    onlock: () => void
    /** Opens the import from another app, `true` for this app's own export. */
    onimport: (ficelle: boolean) => void
    /** Opens restoring a backup from a file. */
    onrestorefile: () => void
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
    | 'feedback'
    | 'testTools'
    | 'about'
  // svelte-ignore state_referenced_locally
  let page: Page = $state(start ?? 'home')
  let query = $state('')

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

  const lookRows = $derived<SettingRow[]>([
    {
      icon: 'palette',
      label: t('settings.appearance'),
      keywords: t('settings.appearanceKeywords'),
      value:
        prefs.mode === 'dark' && prefs.pureBlack
          ? t('settings.pureBlack')
          : t(`settings.theme_${prefs.mode}`),
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
      value: prefs.autoLists ? t('settings.phone.listsOn') : t('settings.phone.listsOff'),
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
      icon: sync.status.state === 'off' && !localSync.summary ? 'offline' : 'synced',
      label: t('settings.sync'),
      keywords: t('settings.syncKeywords'),
      // a device syncs with a server or with its paired devices
      value:
        sync.status.state === 'off' && localSync.summary
          ? t(`local.status.${localSync.summary.key}`, { name: localSync.summary.name })
          : t(`sync.state.${sync.status.state}`),
      run: () => (page = 'sync'),
    },
  ])

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
    ['eye', 'settings.theme', 'appearance'],
    ['backup', 'backups.title', 'notes'],
    ['restore', 'backups.restoreFrom', 'notes'],
    ['import', 'settings.importTitle', 'notes'],
    ['export', 'settings.exportTitle', 'notes'],
    ['palette', 'settings.pureBlack', 'appearance'],
    ['textSize', 'settings.uiSize', 'accessibility'],
    ['textStyle', 'settings.dyslexicFont', 'accessibility'],
    ['list', 'settings.autoLists', 'editor'],
    ['emoji', 'settings.emojiShortcodes', 'editor'],
    ['right', 'settings.arrow', 'editor'],
    ['timer', 'settings.clock', 'language'],
    ['info', 'settings.credits', 'about'],
  ]
  if (updatesHere) inner.push(['info', 'appUpdate.check', 'about'])
  if (updatesHere && channelChoice) inner.push(['info', 'appUpdate.betaVersions', 'about'])
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
    feedback: 'settings.feedback',
    testTools: 'settings.testTools',
    about: 'settings.about',
  }

  function back() {
    page = 'home'
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
    const returning = next === 'home'
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
    <!-- the value under its label: long words in any language keep room -->
    <span class="text">
      <span class="label">{r.label}</span>
      {#if r.on === undefined && r.value}<span class="value">{r.value}</span>{/if}
    </span>
    {#if r.on !== undefined}
      <Switch drawn checked={r.on} />
    {:else if !r.action}
      <Icon name="right" size={16} />
    {/if}
  </button>
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
        <div class="hero">
          <span class="mark"><Knot size={32} /></span>
          <span>
            <b>{brand.displayName}</b>
            <span>{encrypted ? t('settings.phone.encrypted') : t('settings.phone.plain')}</span>
          </span>
        </div>
        <Group title={t('settings.phone.groupLook')}>
          {#each lookRows as r (r.label)}{@render row(r)}{/each}
        </Group>
        {#if privacyRows.length}
          <Group
            title={vault && !encrypted
              ? t('settings.notEncryptedTitle')
              : t('settings.phone.groupPrivacy')}
            note={vault && !encrypted ? t('settings.encryptNote') : undefined}
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
        <Group title={t('settings.sync')}>
          {#each syncRows as r (r.label)}{@render row(r)}{/each}
        </Group>
        <Group>
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
        ><Icon name="back" size={20} /><span>{t('settings.title')}</span></button
      >
      <b>{t(titles[page])}</b>
      <span></span>
    </header>

    <div class="groups page">
      {#if page === 'appearance'}
        <Group title={t('settings.theme')}>
          {#each MODES as mode (mode)}
            <Row
              label={t(`settings.theme_${mode}`)}
              checked={prefs.mode === mode}
              onclick={() => (prefs.mode = mode)}
            />
          {/each}
        </Group>
        <Group note={t('settings.pureBlackHelp')}>
          {@render row({
            icon: 'palette',
            label: t('settings.pureBlack'),
            on: prefs.pureBlack,
            disabled: prefs.mode === 'light',
            run: () => (prefs.pureBlack = !prefs.pureBlack),
          })}
        </Group>
        <Group title={t('settings.tagMark')} note={t('settings.tagMarkHelp')}>
          {#each TAG_MARKS as mark (mark)}
            <Row
              label={t(`settings.tagMark_${mark}`)}
              checked={prefs.tagMark === mark}
              onclick={() => (prefs.tagMark = mark)}
            />
          {/each}
        </Group>
      {:else if page === 'accessibility'}
        <Group title={t('settings.uiSize')} note={t('settings.uiSizeHelp')}>
          {#each UI_SIZES as size (size)}
            <Row
              label={t(`settings.uiSize_${size}`)}
              checked={prefs.uiSize === size}
              onclick={() => (prefs.uiSize = size)}
            />
          {/each}
        </Group>
        <Group>
          {@render row({
            icon: 'pin',
            label: t('settings.haptics'),
            on: prefs.haptics,
            run: () => (prefs.haptics = !prefs.haptics),
          })}
        </Group>
        <Group>
          {@render row({
            icon: 'textStyle',
            label: t('settings.dyslexicFont'),
            on: prefs.dyslexicFont,
            run: () => (prefs.dyslexicFont = !prefs.dyslexicFont),
          })}
          {#if prefs.dyslexicFont && dyslexicStatus.loading}
            <p class="under" role="status">{t('settings.dyslexicFontLoading')}</p>
          {:else if prefs.dyslexicFont && dyslexicStatus.failed}
            <p class="under error" role="alert">{t('settings.dyslexicFontFailed')}</p>
          {/if}
        </Group>
      {:else if page === 'editor'}
        <Group>
          {@render row({
            icon: 'list',
            label: t('settings.autoLists'),
            on: prefs.autoLists,
            run: () => (prefs.autoLists = !prefs.autoLists),
          })}
          {@render row({
            icon: 'emoji',
            label: t('settings.emojiShortcodes'),
            on: prefs.emojiShortcodes,
            run: () => (prefs.emojiShortcodes = !prefs.emojiShortcodes),
          })}
        </Group>
        <Group title={t('settings.arrow')} typed>
          <div class="inside"><ArrowPicker /></div>
        </Group>
      {:else if page === 'language'}
        <Group
          title={t('settings.language')}
          note={t('settings.languageMachine', { url: LOCALES_URL })}
        >
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
        <DataSettings {settings} {onimport} {onrestorefile} {onreset} />
      {:else if page === 'feedback'}
        <FeedbackSettings />
      {:else if page === 'testTools'}
        <TestTools />
      {:else if page === 'about'}
        <AboutSettings phone />
      {/if}
    </div>
  {/if}

  {#if settings.message}<p class="message" role="status">{settings.message}</p>{/if}
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
    border-radius: var(--radius-popover);
    background: var(--color-surface-2);
    color: var(--color-text-muted);
  }
  /* the field has no box of its own, the bar around it shows the focus */
  .search:focus-within {
    outline: 2px solid var(--color-accent);
    outline-offset: -2px;
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
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  .hero .mark {
    --knot-behind: var(--color-surface-2);
    display: grid;
    flex: none;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: var(--radius-control);
    background: var(--knot-behind);
  }
  :global(:root[data-black='true']) .hero .mark {
    --knot-behind: transparent;
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
  /* pure black means black: what would be a shade of grey is an outline */
  :global(:root[data-black='true']) .search,
  :global(:root[data-black='true']) .hero,
  :global(:root[data-black='true']) .screen :global(section.group > .panel),
  :global(:root[data-black='true']) .screen :global(.settings-panel) {
    border: 1px solid var(--color-border);
    background: transparent;
  }
  :global(:root[data-black='true']) .tile {
    background: transparent;
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
    border-radius: var(--radius-popover);
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
    min-height: 52px;
    font-size: 16px;
  }
  .screen :global(.settings-row .help) {
    font-size: 13px;
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
    min-height: 52px;
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
    left: calc(var(--space-3) * 2 + 30px);
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
    width: 30px;
    height: 30px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: var(--radius-control);
    /* one quiet tile for every row, the icon in the accent: the colour
       says "a setting", not which kind */
    background: var(--color-surface-2);
    color: var(--color-accent);
  }
  .tile :global(.icon) {
    color: inherit;
  }
  /* with a large text size the value gives way first, cut short, and the
     label wraps if it still has no room: never drawn over each other */
  .text {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2) 0;
  }
  .label {
    overflow-wrap: anywhere;
    hyphens: auto;
  }
  .value {
    color: var(--color-text-muted);
    font-size: 14px;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }
  /* a value in a shared row (About): nothing else beside it to give way to */
  :global(.settings-row) .value {
    max-width: none;
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
