<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- One section of the settings (sections.svelte.ts), in either layout: on
     the desktop (`narrow` false) rows with their control on the right, on
     the phone rows that fold their choices under them or open a page. -->
<script lang="ts">
  import { tick, type ComponentProps } from 'svelte'
  import { brand } from '@app/brand'
  import { MIN_PASSWORD } from '../lib/passwords'
  import PasswordField from '../lib/PasswordField.svelte'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { overlay, type MenuItem } from '../lib/overlay.svelte'
  import { prefs, shortcutFor } from '../lib/prefs.svelte'
  import { applyQuickShortcut, DEFAULT_QUICK_SHORTCUT, globalBindingOf } from '../lib/quickNote'
  import { ACTIONS, bindingOf, displayBinding, type ActionId } from '../lib/shortcuts'
  import RecoveryKey from '../screens/RecoveryKey.svelte'
  import TestTools from '../devreport/TestTools.svelte'
  import AboutSettings from './AboutSettings.svelte'
  import ArrowPicker from './ArrowPicker.svelte'
  import DataSettings from './DataSettings.svelte'
  import FeedbackSettings from './FeedbackSettings.svelte'
  import FontLoad from './FontLoad.svelte'
  import SyncSettings from './SyncSettings.svelte'
  import UpdateCheck from './UpdateCheck.svelte'
  import {
    quickNote,
    showing,
    type ChoiceItem,
    type FontLoading,
    type Item,
    type ItemGroup,
    type Section,
  } from './sections.svelte'
  import type { DataActions, SettingsState } from './settingsState.svelte'
  import Group from './ui/Group.svelte'
  import ListRow from './ui/ListRow.svelte'
  import Row from './ui/Row.svelte'
  import Switch from './ui/Switch.svelte'

  let {
    section,
    narrow = false,
    rows,
    settings,
    onopen,
    data,
  }: {
    section: Section
    narrow?: boolean
    /** Only these of its rows, found by a search on the phone. */
    rows?: Item[]
    settings: SettingsState
    /** Opens another section, or on the phone a choice's own page by its
     * label. */
    onopen: (to: string) => void
    /** What the Data section opens over the settings. */
    data: DataActions
  } = $props()

  const vault = $derived(settings.vault)

  interface Part {
    title?: string
    note?: string
    items: Item[]
    /** A choice laid out as a list of its own, under its name. */
    list?: ChoiceItem
  }
  const look = (item: ChoiceItem) => (narrow ? item.narrow : item.wide)

  /** A group as it shows: a choice listed under its own title stands apart,
   * on the phone a `split` group is a group for each row. */
  function parts(group: ItemGroup): Part[] {
    const out: Part[] = []
    let rows: Item[] = []
    const flush = () => {
      if (rows.length) out.push({ items: rows })
      rows = []
    }
    for (const item of group.items.filter(showing)) {
      if (item.kind === 'choice' && look(item) === 'list') {
        flush()
        out.push({ items: [item], list: item })
      } else {
        rows.push(item)
        if (narrow && group.split) flush()
      }
    }
    flush()
    const title = narrow ? (group.narrowTitle ?? group.title) : group.title
    const first = out[0]
    const last = out.at(-1)
    if (first && !first.list && title) first.title = t(title)
    if (last) last.note = group.note?.()
    return out
  }

  // a page shows one choice list at a time, the others folded into their
  // rows, so it stays short
  let unfolded: string | null = $state(null)
  const foldId = (item: Item) => `choices-${item.label.split('.').at(-1)}`
  function fold(item: Item) {
    unfolded = unfolded === foldId(item) ? null : foldId(item)
  }
  /** A choice made: its list folds and the focus goes back to its row. */
  function chosen() {
    const list = unfolded
    unfolded = null
    void tick().then(() =>
      document.querySelector<HTMLElement>(`[aria-controls="${list}"]`)?.focus(),
    )
  }

  /** An item's row on the phone. */
  function narrowRow(item: Item): ComponentProps<typeof ListRow> {
    const row = { icon: item.icon, label: t(item.label), value: item.value?.() }
    if (item.kind === 'switch')
      return { ...row, on: item.on(), onclick: () => item.set(!item.on()) }
    if (item.kind === 'action') return { ...row, action: !item.opens, onclick: item.run }
    if (item.kind === 'password')
      return { ...row, onclick: () => void settings.changePasswordAsked() }
    if (item.kind === 'recovery')
      return {
        ...row,
        label: vault?.recovery ? t('recovery.heading') : t('recovery.create'),
        value: vault?.recovery ? t('settings.phone.keyCreated') : undefined,
        onclick: recoveryMenu,
      }
    if (item.kind === 'choice') row.value ??= item.options().find((o) => o.picked)?.label
    if (item.kind === 'choice' && item.narrow === 'page')
      return { ...row, onclick: () => onopen(item.label) }
    return {
      ...row,
      typed: item.kind === 'arrow',
      folds: foldId(item),
      open: unfolded === foldId(item),
      onclick: () => fold(item),
    }
  }

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

  let recording: ActionId | null = $state(null)
  let recordingQuick = $state(false)

  /** The system may refuse a global shortcut another app holds. */
  async function setQuick(binding: string | null) {
    quickNote.taken = false
    if (await applyQuickShortcut(binding)) prefs.quickNoteShortcut = binding
    else {
      quickNote.taken = true
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

<svelte:window onkeydowncapture={record} />

{#snippet fontLoad(load: FontLoading)}
  <FontLoad font={load.font} loading={load.loading} error={load.error} onretry={load.retry} />
{/snippet}

{#snippet quickNoteRow(item: Item)}
  <Row label={t(item.label)} icon={narrow ? item.icon : undefined}>
    {#if prefs.quickNoteShortcut}
      <button class="btn key" onclick={() => (recordingQuick = true)}>
        {recordingQuick ? t('settings.pressKeys') : displayBinding(prefs.quickNoteShortcut)}
      </button>
    {/if}
    <Switch
      label={t(item.label)}
      checked={prefs.quickNoteShortcut !== null}
      onchange={(on) => void setQuick(on ? DEFAULT_QUICK_SHORTCUT : null)}
    />
  </Row>
{/snippet}

{#snippet narrowItem(item: Item)}
  {#if item.kind === 'quickNote'}
    {@render quickNoteRow(item)}
  {:else}
    <ListRow {...narrowRow(item)} />
  {/if}
  {#if unfolded === foldId(item)}
    <div id={foldId(item)} class="choices" class:picker={item.kind === 'arrow'}>
      {#if item.kind === 'choice'}
        {#each item.options() as option (option.label)}
          <Row
            label={option.label}
            help={option.help}
            checked={option.picked}
            onclick={() => {
              option.pick()
              chosen()
            }}
          />
        {/each}
      {:else if item.kind === 'tools'}
        {#each item.switches() as tool (tool.icon)}<ListRow {...narrowRow(tool)} />{/each}
      {:else}
        <ArrowPicker />
      {/if}
    </div>
  {/if}
  {@const load = item.load?.()}
  {#if load && (load.loading || load.error)}
    <div class="under">{@render fontLoad(load)}</div>
  {/if}
{/snippet}

{#snippet wideItem(item: Item)}
  {#if item.kind === 'switch'}
    <Row label={t(item.label)}>
      <Switch label={t(item.label)} checked={item.on()} onchange={item.set} />
    </Row>
  {:else if item.kind === 'choice' && item.wide === 'select'}
    {@const options = item.options()}
    <Row label={t(item.label)}>
      <select
        class="field compact {item.width ?? ''}"
        aria-label={t(item.label)}
        value={options.findIndex((o) => o.picked)}
        onchange={(e) => options[Number(e.currentTarget.value)]?.pick()}
      >
        {#each options as option, i (option.label)}
          <option value={i}>{[option.label, option.help].filter(Boolean).join(', ')}</option>
        {/each}
      </select>
    </Row>
  {:else if item.kind === 'choice'}
    <Row label={t(item.label)}>
      <div class="segmented" role="radiogroup" aria-label={t(item.label)}>
        {#each item.options() as option (option.label)}
          <label class:checked={option.picked}>
            <input
              type="radio"
              name={item.label}
              checked={option.picked}
              onchange={option.pick}
            />{option.label}
          </label>
        {/each}
      </div>
    </Row>
  {:else if item.kind === 'action'}
    <Row label={t(item.wideLabel ?? item.label)}>
      <button class="btn" class:primary={item.primary} onclick={item.run}
        >{#if item.buttonIcon}<Icon name={item.buttonIcon} size={16} />{/if}{t(item.button)}</button
      >
    </Row>
  {:else if item.kind === 'tools'}
    <Row
      label={t(item.label)}
      expanded={unfolded === foldId(item)}
      controls={foldId(item)}
      onclick={() => fold(item)}
    >
      <span class="state">{item.value?.()}</span>
    </Row>
    {#if unfolded === foldId(item)}
      <div id={foldId(item)} class="folded">
        {#each item.switches() as tool (tool.icon)}
          <Row label={t(tool.label)}>
            <Switch label={t(tool.label)} checked={tool.on()} onchange={tool.set} />
          </Row>
        {/each}
      </div>
    {/if}
  {:else if item.kind === 'arrow'}
    <Row label={t(item.label)} typed>
      <ArrowPicker />
    </Row>
  {:else if item.kind === 'quickNote'}
    {@render quickNoteRow(item)}
  {:else if item.kind === 'recovery' && settings.recoveryCode}
    <div class="inside">
      <RecoveryKey code={settings.recoveryCode} done={() => settings.recoveryShown()} level="h3" />
    </div>
  {:else if item.kind === 'recovery'}
    <Row label={t(item.label)}>
      <span class="state"
        >{vault?.recovery ? t('settings.phone.keyCreated') : t('settings.phone.keyNone')}</span
      >
      <button class="btn" onclick={() => settings.createRecovery()}
        >{vault?.recovery ? t('recovery.replace') : t('recovery.create')}</button
      >
      {#if vault?.recovery}
        <button class="btn danger" onclick={() => settings.removeRecovery()}
          >{t('recovery.remove')}</button
        >
      {/if}
    </Row>
  {:else if item.kind === 'password'}
    <Row label={t('settings.password')}>
      <button class="btn" onclick={() => (changingPassword = !changingPassword)}
        >{changingPassword ? t('common.cancel') : t('settings.change')}</button
      >
    </Row>
    {#if changingPassword}
      <form class="inside after" onsubmit={changePassword}>
        <input type="text" autocomplete="username" value={brand.displayName} hidden readonly />
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
{/snippet}

{#if rows}
  {#each rows as item (item.label)}{@render narrowItem(item)}{/each}
{:else}
  {#each section.groups as group, g (g)}
    {#each parts(group) as part, p (p)}
      {#if part.list}
        {@const item = part.list}
        {#snippet choices()}
          {#each item.options() as option (option.label)}
            <Row
              label={option.label}
              help={option.help}
              checked={option.picked}
              onclick={option.pick}
            />
          {/each}
          {#if item.extra?.shown()}
            {#if narrow}
              <Row label={t(item.extra.label)} opens onclick={item.extra.run} />
            {:else}
              <Row label={t(item.extra.label)}>
                <button class="btn" onclick={item.extra.run}>{t(item.extra.label)}</button>
              </Row>
            {/if}
          {/if}
        {/snippet}
        {#if narrow}
          <Group title={t(item.label)}>{@render choices()}</Group>
        {:else}
          <div role="radiogroup" aria-label={t(item.label)}>
            <Group title={t(item.label)}>{@render choices()}</Group>
          </div>
        {/if}
      {:else}
        <Group title={part.title} note={part.note}>
          {#each part.items as item (item.label)}
            {#if narrow}{@render narrowItem(item)}{:else}{@render wideItem(item)}{/if}
          {/each}
        </Group>
        {#if !narrow}
          {#each part.items as item (item.label)}
            {@const load = item.load?.()}
            {#if load}{@render fontLoad(load)}{/if}
          {/each}
        {/if}
      {/if}
    {/each}
  {/each}

  {#if section.id === 'shortcuts'}
    <Group>
      {#each ACTIONS as action (action)}
        <Row label={t(`shortcuts.${action}`)}>
          <button
            class="btn key"
            aria-label={`${t(`shortcuts.${action}`)}, ${displayBinding(shortcutFor(action))}`}
            onclick={() => (recording = action)}
          >
            {recording === action ? t('settings.pressKeys') : displayBinding(shortcutFor(action))}
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
  {:else if section.id === 'sync'}
    <SyncSettings
      phone={narrow}
      unencrypted={!!vault && !vault.encrypted}
      onsecurity={() => onopen('security')}
    />
  {:else if section.id === 'data'}
    <DataSettings {settings} {...data} />
  {:else if section.id === 'feedback'}
    <FeedbackSettings />
  {:else if section.id === 'testTools'}
    <TestTools />
  {:else if section.id === 'updates'}
    <UpdateCheck />
  {:else if section.id === 'about'}
    <AboutSettings phone={narrow} />
  {/if}
{/if}

<style>
  /* the choices opened under their row on the phone, a hairline between
     like any row */
  .choices {
    position: relative;
  }
  .choices::before {
    content: '';
    position: absolute;
    top: 0;
    left: var(--space-4);
    right: var(--space-4);
    border-top: 1px solid var(--color-border);
  }
  .picker {
    padding: var(--space-2) var(--space-4);
  }
  /* a font's download under its row on the phone */
  .under {
    margin: 0;
    padding: 0 var(--space-4) var(--space-2);
    color: var(--color-text-muted);
    font-size: 13px;
  }
  /* the switches folded under their row on the desktop, a hairline above
     like any row */
  .folded {
    position: relative;
  }
  .folded::before {
    content: '';
    position: absolute;
    top: 0;
    left: var(--space-3);
    right: 0;
    border-top: 1px solid var(--color-border);
  }
  /* what opens inside a group, under its row: a form, the new key */
  .inside {
    display: grid;
    justify-items: start;
    gap: var(--space-2);
    padding: var(--space-3);
  }
  /* opened under its row: the row above gives the space */
  .inside.after {
    padding-top: 0;
  }
  .state {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  /* wide enough for the longest default (Ctrl+Backspace), so the keys
     share a left edge */
  .key {
    min-width: 130px;
    justify-content: center;
    font-family: var(--font-ui);
  }
  /* the font boxes line up, wide enough for the default's two names */
  select.field.compact.font-pick {
    width: 22rem;
    max-width: 100%;
  }
  /* the language and time format boxes line up, whatever their options */
  select.field.compact.same-width {
    width: 15rem;
    max-width: 100%;
  }
</style>
