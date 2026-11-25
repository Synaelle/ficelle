<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Settings dialog. A search box filters sections by their translated text.
     Phones get their own full screen layout of the same settings,
     PhoneSettings. -->
<script lang="ts">
  import ErrorText from '../lib/ErrorText.svelte'
  import { modal } from '../lib/modal'
  import { onMount, untrack } from 'svelte'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import BackupDialog from './BackupDialog.svelte'
  import ResetDialog from './ResetDialog.svelte'
  import PhoneSettings from './PhoneSettings.svelte'
  import SettingsSection from './SettingsSection.svelte'
  import { sectionLabel, settingsInside, settingsSections, type SectionId } from './sections.svelte'
  import { SettingsState } from './settingsState.svelte'
  import type ImportScreen from './ImportDialog.svelte'
  import { loadPart } from '../lib/pageParts'

  let {
    close,
    onlock,
    start,
  }: { close: () => void; onlock: () => void; start?: 'sync' | 'updates' } = $props()

  const settings = new SettingsState()
  onMount(() => void settings.refresh())
  const sections = settingsSections(settings, () => onlock())

  /** At phone width the settings fill the screen with their own layout. */
  const PHONE = 640
  let width = $state(window.innerWidth)
  const phone = $derived(width <= PHONE)
  // svelte-ignore state_referenced_locally
  let section: SectionId = $state(start ?? 'appearance')
  const current = $derived(sections.find((s) => s.id === section))
  /** The import dialog open, for this app's own export or another app's. */
  let importing = $state<'ficelle' | 'other' | null>(null)
  /** The importers of every other app load only when an import starts. */
  let ImportDialog = $state<typeof ImportScreen | null>(null)
  async function startImport(ficelle: boolean) {
    ImportDialog ??= (await loadPart(() => import('./ImportDialog.svelte')))?.default ?? null
    if (ImportDialog) importing = ficelle ? 'ficelle' : 'other'
  }
  let resetting = $state(false)
  let restoringFile = $state(false)
  const data = {
    onimport: (ficelle: boolean) => void startImport(ficelle),
    onrestorefile: () => (restoringFile = true),
    onreset: () => (resetting = true),
  }
  let filter = $state('')
  const visible = $derived.by(() => {
    const words = filter.trim().toLowerCase()
    // a setting found by its own name, as on the phone, shows its section
    const inside = settingsInside(sections).filter(([, label]) =>
      t(label).toLowerCase().includes(words),
    )
    return sections.filter(
      (s) =>
        !words ||
        t(sectionLabel(s.id)).toLowerCase().includes(words) ||
        t(`settings.${s.id}Keywords`).toLowerCase().includes(words) ||
        inside.some(([, , id]) => id === s.id),
    )
  })
  function open(next: string) {
    section = next as SectionId
    settings.message = ''
  }
  // a search shows its first match, not the page that was open before it
  $effect(() => {
    const first = filter.trim() ? visible[0]?.id : undefined
    if (first) untrack(() => section !== first && open(first))
  })
</script>

<svelte:window bind:innerWidth={width} />

{#if phone}
  <PhoneSettings {sections} {settings} {close} {start} {data} />
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
        <input
          class="field"
          type="search"
          placeholder={t('settings.search')}
          bind:value={filter}
          onkeydown={(e) => e.key === 'Enter' && visible[0] && open(visible[0].id)}
        />
        {#each visible as s (s.id)}
          <button class:active={section === s.id} onclick={() => open(s.id)}
            ><span class="tile"><Icon name={s.icon} size={14} /></span>{t(
              sectionLabel(s.id),
            )}</button
          >
        {/each}
      </nav>

      <div class="content">
        <header>
          <h2>{t(sectionLabel(section))}</h2>
          <button class="icon-btn" aria-label={t('common.close')} onclick={close}
            ><Icon name="close" /></button
          >
        </header>

        <div class="settings-body">
          {#if current}
            <SettingsSection section={current} {settings} onopen={open} {data} />
          {/if}
        </div>

        {#if settings.message}<p class="message" role="status">
            <ErrorText error={settings.message} />
          </p>{/if}
      </div>
    </div>
  </div>
{/if}

{#if resetting}
  <ResetDialog close={() => (resetting = false)} />
{/if}

{#if importing && ImportDialog}
  <ImportDialog ficelle={importing === 'ficelle'} close={() => (importing = null)} />
{/if}

{#if restoringFile}
  <BackupDialog mode="restore" close={() => (restoringFile = false)} />
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
    /* an edge outside the box: a zoomed border is snapped to whole device
       pixels, which left the pages a fraction taller than their window and
       scrolling by 1 px */
    box-shadow: 0 0 0 1px var(--color-border-strong);
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
    font-size: var(--title-dialog);
  }
  /* a menu in a settings row, as wide as its longest choice */
  .content :global(.field.compact) {
    width: auto;
    min-width: 160px;
    height: 30px;
  }
  .message {
    margin: var(--space-5) 0 0;
  }
</style>
