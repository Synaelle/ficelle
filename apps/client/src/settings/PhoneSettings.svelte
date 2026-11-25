<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Settings on phones, grouped lists like a phone's own settings: rows flat
     on the page with hairlines between, a title over each group, icon tiles,
     the current value on the right, a short note under groups.
     Every row is one tap from the first screen, details open a page. -->
<script lang="ts">
  import ErrorText from '../lib/ErrorText.svelte'
  import { brand } from '@app/brand'
  import Knot from '@app/ui/Knot.svelte'
  import Group from './ui/Group.svelte'
  import ListRow from './ui/ListRow.svelte'
  import Row from './ui/Row.svelte'
  import { tick, untrack } from 'svelte'
  import { onBack } from '../lib/back'
  import { modal } from '../lib/modal'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import RecoveryKey from '../screens/RecoveryKey.svelte'
  import SettingsSection from './SettingsSection.svelte'
  import {
    sectionLabel,
    settingsInside,
    showing,
    type ChoiceItem,
    type Item,
    type Section,
  } from './sections.svelte'
  import type { DataActions, SettingsState } from './settingsState.svelte'

  let {
    sections,
    settings,
    close,
    data,
    start,
  }: {
    sections: Section[]
    settings: SettingsState
    close: () => void
    data: DataActions
    start?: 'sync' | 'updates'
  } = $props()

  /** The first screen, a section or a choice's own page, by its label. */
  // svelte-ignore state_referenced_locally
  let page: string = $state(start ?? 'home')
  let query = $state('')

  const encrypted = $derived(settings.vault?.encrypted === true)
  const section = $derived(sections.find((s) => s.id === page))
  const choice = $derived(
    sections
      .flatMap((s) => s.groups.flatMap((g) => g.items))
      .find((item): item is ChoiceItem => item.kind === 'choice' && item.label === page),
  )

  /** The first screen's groups: rows opening the sections, and the rows of
   * those that show their own there. */
  const blocks = $derived.by(() => {
    const out: { home: Section['home']; sections: Section[] }[] = []
    for (const s of sections.filter((s) => s.home)) {
      const last = out.at(-1)
      if (last && last.home === s.home) last.sections.push(s)
      else out.push({ home: s.home, sections: [s] })
    }
    return out
  })
  const HOME_TITLES: Partial<Record<NonNullable<Section['home']>, string>> = {
    look: 'settings.phone.groupLook',
    sync: 'settings.sync',
  }

  interface Found {
    icon: IconName
    label: string
    value?: string
    keywords?: string
    run: () => void
  }
  const sectionRow = (s: Section): Found => ({
    icon: s.narrowIcon?.() ?? s.icon,
    label: t(sectionLabel(s.id)),
    value: s.value?.(),
    keywords: t(`settings.${s.id}Keywords`),
    run: () => (page = s.id),
  })

  /** What a search finds: the first screen's rows, the security rows
   * among them, then the settings inside the pages, whose page shows as the
   * value. A row reached two ways shows once. */
  const matches = $derived.by(() => {
    const q = query.trim().toLowerCase()
    const found = (label: string, keywords?: string) =>
      label.toLowerCase().includes(q) || !!keywords?.toLowerCase().includes(q)
    const seen: string[] = []
    const once = (label: string) => !seen.includes(label) && !!seen.push(label)
    const rows = (list: typeof blocks) =>
      list
        .flatMap((block) => block.sections.map(sectionRow))
        .filter((r) => found(r.label, r.keywords) && once(r.label))
    const at = blocks.findIndex((block) => block.home === 'inline')
    const before = rows(blocks.slice(0, at))
    // the desktop's Security section, found by its words
    const inline = (blocks[at]?.sections ?? []).flatMap((s) =>
      s.groups
        .flatMap((g) => g.items.filter(showing))
        .filter(
          (item) => found(t(item.label), t(`settings.${s.id}Keywords`)) && once(t(item.label)),
        ),
    )
    const after = rows(blocks.slice(at + 1)).concat(
      settingsInside(sections.filter((s) => s.home))
        .map(([icon, label, id]) => ({
          icon,
          label: t(label),
          value: t(sectionLabel(id)),
          run: () => (page = id),
        }))
        .filter((r) => found(r.label) && once(r.label)),
    )
    return { before, inline, after, none: !before.length && !inline.length && !after.length }
  })
  const security = $derived(sections.find((s) => s.home === 'inline'))

  function open(to: string) {
    if (to === 'security') back()
    else page = to
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
  const places: Record<string, number> = {}
  // the page as last drawn, apart from `page` which already moved on
  let shown = untrack(() => page)
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

{#snippet row(r: Found)}
  <ListRow icon={r.icon} label={r.label} value={r.value} onclick={r.run} />
{/snippet}

{#snippet sectionRows(s: Section, rows?: Item[])}
  <SettingsSection section={s} narrow {rows} {settings} onopen={open} {data} />
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
        {#if matches.none}
          <p class="empty">{t('list.noResults')}</p>
        {:else}
          <Group>
            {#each matches.before as r (r.label)}{@render row(r)}{/each}
            {#if security && matches.inline.length}{@render sectionRows(
                security,
                matches.inline,
              )}{/if}
            {#each matches.after as r (r.label)}{@render row(r)}{/each}
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
        {#each blocks as block, b (b)}
          {#if block.home === 'inline'}
            {#each block.sections as s (s.id)}{@render sectionRows(s)}{/each}
          {:else}
            {@const title = HOME_TITLES[block.home ?? 'more']}
            <Group title={title ? t(title) : undefined}>
              {#each block.sections as s (s.id)}{@render row(sectionRow(s))}{/each}
            </Group>
          {/if}
        {/each}
      {/if}
    </div>
  {:else}
    <header class="bar">
      <button class="back" onclick={back}
        ><Icon name="back" size={20} /><span>{t('settings.title')}</span></button
      >
      <b>{t(section ? sectionLabel(section.id) : page)}</b>
      <span></span>
    </header>

    <div class="groups page">
      {#if section}
        {@render sectionRows(section)}
      {:else if choice}
        <Group>
          {#each choice.options() as option (option.label)}
            <Row
              label={option.label}
              help={option.help}
              checked={option.picked}
              onclick={option.pick}
            />
          {/each}
          {#if choice.extra?.shown()}
            <Row label={t(choice.extra.label)} opens onclick={choice.extra.run} />
          {/if}
        </Group>
      {/if}
    </div>
  {/if}

  {#if settings.message}<p class="message" role="status">
      <ErrorText error={settings.message} />
    </p>{/if}
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
  /* the app's name and state on the page itself, the mark in a tile like the
     rows' icons */
  .hero {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) 0;
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
  :global(:root[data-black='true']) .search {
    border: 1px solid var(--color-border-strong);
    background: transparent;
  }
  :global(:root[data-black='true']) .screen :global(.settings-row > .tile) {
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
     no panel, the rows flat on the page, as wide as the screen so a press
     shows across it, their text on the page's margin like the captions.
     Panels drawn by the pages themselves (.settings-panel) go the same way */
  .screen :global(section.group > .panel),
  .screen :global(.settings-panel) {
    margin-inline: calc(var(--space-4) * -1);
    border: 0;
    border-radius: 0;
    background: none;
  }
  .screen :global(.settings-panel) {
    padding-inline: var(--space-4);
  }
  .screen :global(section.group > h3) {
    margin: 0;
    color: var(--color-text-muted);
    font-size: 13px;
    font-weight: 600;
  }
  .screen :global(section.group > .note) {
    margin: 0;
    font-size: 13px;
  }
  /* a page's opening line lines up with the captions */
  .groups > :global(.settings-help),
  .screen :global(.sync-form > .settings-help) {
    margin: 0;
  }
  /* a name with its help line fits the height of a row without one */
  .screen :global(.settings-row),
  .screen :global(.field-row) {
    min-height: 52px;
    padding: var(--space-1) var(--space-4);
    font-size: 16px;
  }
  .screen :global(.settings-row .help) {
    font-size: 13px;
  }
  /* hairlines between rows on the page's margins */
  .screen :global(.settings-row + .settings-row)::before,
  .screen :global(.field-row + .field-row)::before,
  .screen :global(.settings-row + .field-row)::before,
  .screen :global(.field-row + .settings-row)::before {
    left: var(--space-4);
    right: var(--space-4);
  }
  /* on pure black a field among the flat rows is lost in the black: it
     takes its own edge, and the hairlines beside it would only double it */
  :global(:root[data-black='true']) .screen :global(.field-row > .field),
  :global(:root[data-black='true']) .screen :global(.field-row > * > .field) {
    padding: 0 var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-control);
  }
  :global(:root[data-black='true']) .screen :global(.field-row textarea.field) {
    padding: var(--space-2) var(--space-3);
  }
  :global(:root[data-black='true']) .screen :global(.field-row .field:focus) {
    border-color: var(--color-accent);
  }
  :global(:root[data-black='true']) .screen :global(.field-row:has(.field:focus-visible)) {
    box-shadow: none;
  }
  :global(:root[data-black='true']) .screen :global(.field-row + .field-row)::before,
  :global(:root[data-black='true']) .screen :global(.settings-row + .field-row)::before,
  :global(:root[data-black='true']) .screen :global(.field-row + .settings-row)::before {
    content: none;
  }
  /* a control too wide beside the name goes under it, not under the icon */
  .screen :global(.settings-row > .main) {
    display: flex;
    flex: 1 1 0;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
    min-width: 0;
  }
  .screen :global(.settings-row .control) {
    padding-block: var(--space-1);
  }
  /* a shared row with an icon (settings/ui/Row) gets the same tile, and its
     hairline starts after it. The tile is level with the first line, so a
     row whose control went under its name keeps it beside the name */
  .screen :global(.settings-row > .tile) {
    display: grid;
    align-self: flex-start;
    width: 30px;
    height: 30px;
    flex: none;
    margin-top: calc((52px - 30px) / 2 - var(--space-1));
    place-items: center;
    border-radius: var(--radius-control);
    background: var(--color-surface-2);
    color: var(--color-accent);
  }
  .screen :global(.settings-row:has(> .tile))::before {
    left: calc(var(--space-4) + var(--space-3) + 30px);
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
