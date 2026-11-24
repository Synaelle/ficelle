<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Every attachment, in place of the note list when Attachments is chosen
     in the sidebar or, on a phone, in the sheet under the title: name, type,
     size, its note and when it was added, newest first, with a filter by
     name and a sort. A file opens its note, its button opens the file
     itself, a file on its way shows how far it got. Notes in the trash are
     left out. -->
<script lang="ts">
  import { onDestroy } from 'svelte'
  import { openAttachment } from '../lib/attachments'
  import { backend, type NoteAttachment } from '../lib/backend'
  import { bytes } from '../lib/format'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import { FILES_CHANGED, sync } from '../lib/sync.svelte'
  import { movingOf } from '../lib/transfers'
  import { workspace } from '../lib/workspace.svelte'
  import FileProgress from './FileProgress.svelte'
  import TagSheet from './TagSheet.svelte'

  let { narrow, onsettings }: { narrow: boolean; onsettings: () => void } = $props()

  let all: NoteAttachment[] = $state([])
  let filter = $state('')
  let filterInput: HTMLInputElement | undefined = $state()
  let sort = $state<'newest' | 'largest' | 'name'>('newest')
  /** The file whose note was opened from here: the others of that note stay
   * unmarked. */
  let chosen: string | null = $state(null)
  let picking = $state(false)

  async function load() {
    try {
      all = await backend.allAttachments()
    } catch {
      all = []
    }
  }
  void load()
  window.addEventListener(FILES_CHANGED, load)
  onDestroy(() => window.removeEventListener(FILES_CHANGED, load))

  const shown = $derived.by(() => {
    const words = filter.trim().toLowerCase()
    const list = all.filter(
      (f) =>
        !words ||
        f.fileName.toLowerCase().includes(words) ||
        f.noteTitle.toLowerCase().includes(words),
    )
    if (sort === 'largest') return [...list].sort((a, b) => b.size - a.size)
    if (sort === 'name') return [...list].sort((a, b) => a.fileName.localeCompare(b.fileName))
    return list
  })
  const total = $derived(all.reduce((sum, f) => sum + f.size, 0))
  const day = (ms: number) =>
    new Intl.DateTimeFormat(currentLocale(), { dateStyle: 'medium' }).format(ms)

  function show(file: NoteAttachment) {
    chosen = file.id
    void workspace.open(file.noteId)
  }

  async function open(file: NoteAttachment) {
    try {
      await openAttachment(file)
    } catch (err) {
      workspace.report(err, 'FILES')
    }
  }
</script>

<section
  class="list"
  class:narrow
  aria-label={t('files.allTitle')}
  style:width={narrow ? undefined : `${prefs.listWidth}px`}
>
  <header>
    {#if narrow}
      <h1 class="visually-hidden">{t('files.allTitle')}</h1>
      <button
        class="scope"
        aria-haspopup="dialog"
        aria-expanded={picking}
        onclick={() => (picking = true)}
        ><span class="leaf">{t('files.allTitle')}</span><Icon name="down" size={18} /></button
      >
    {:else}
      <h1>{t('files.allTitle')}</h1>
    {/if}
    {#if narrow}
      <button class="icon-btn" aria-label={t('sidebar.settings')} onclick={() => onsettings()}>
        <Icon name="settings" />
      </button>
    {/if}
  </header>
  <div class="tools">
    <label class="search">
      <Icon name="search" size={16} />
      <span class="visually-hidden">{t('files.filter')}</span>
      <input
        bind:this={filterInput}
        type="search"
        placeholder={t('files.filter')}
        bind:value={filter}
      />
      {#if filter}
        <button
          type="button"
          class="icon-btn"
          aria-label={t('files.clearFilter')}
          title={t('files.clearFilter')}
          onclick={() => {
            filter = ''
            filterInput?.focus()
          }}
        >
          <Icon name="close" size={14} />
        </button>
      {/if}
    </label>
    <select class="sort" bind:value={sort} aria-label={t('files.sort')}>
      <option value="newest">{t('files.sort_newest')}</option>
      <option value="largest">{t('files.sort_largest')}</option>
      <option value="name">{t('files.sort_name')}</option>
    </select>
  </div>
  {#if shown.length}
    <ul>
      {#each shown as file (file.id)}
        {@const moving = movingOf(sync.moving, file.sha256)}
        <li>
          <button
            class="row"
            class:active={chosen === file.id && workspace.current?.id === file.noteId}
            onclick={() => show(file)}
          >
            <span class="top">
              <Icon name={file.mimeType.startsWith('image/') ? 'image' : 'attachment'} size={14} />
              <b>{file.fileName}</b>
              <span class="muted size">{bytes(file.size)}</span>
            </span>
            <!-- the type is in the name's ending and the icon -->
            <span class="meta muted">
              <span class="note">{file.noteTitle || t('note.untitled')}</span>
              <span>{day(file.createdAt)}</span>
            </span>
            {#if moving}<FileProgress file={moving} />{/if}
          </button>
          <button
            class="icon-btn"
            aria-label={t('files.openNamed', { name: file.fileName })}
            title={t('files.open')}
            onclick={() => open(file)}><Icon name="openFile" size={16} /></button
          >
        </li>
      {/each}
    </ul>
  {:else}
    <p class="muted empty">{all.length ? t('files.noMatch') : t('files.none')}</p>
  {/if}
  {#if all.length}
    <footer class="muted">{t('files.allCount', { count: all.length, size: bytes(total) })}</footer>
  {/if}
  {#if picking}
    <TagSheet close={() => (picking = false)} />
  {/if}
</section>

<style>
  .list {
    flex: none;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
    background: var(--color-bg);
    border-right: 1px solid var(--color-border);
  }
  .list.narrow {
    width: 100%;
    border-right: 0;
  }
  /* the same as the note list's header */
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    /* the note list's buttons set its height, this one has none on a desktop */
    min-height: 52px;
    padding: var(--space-3) var(--space-3) var(--space-2) var(--space-4);
  }
  h1 {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: var(--title-screen);
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope {
    min-width: 0;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 44px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font-size: 18px;
    font-weight: 650;
  }
  .scope :global(svg) {
    flex: none;
    color: var(--color-text-muted);
  }
  /* the same as the notes' search and sort */
  .tools {
    display: flex;
    gap: var(--space-2);
    padding: 0 var(--space-4) var(--space-2);
  }
  .search {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 32px;
    padding: 0 var(--space-2);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-control);
    background: var(--color-surface);
    color: var(--color-text-muted);
  }
  .search:focus-within {
    border-color: var(--color-accent);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--color-text);
  }
  .search input::-webkit-search-cancel-button {
    display: none;
  }
  .search .icon-btn {
    width: 24px;
    height: 24px;
    margin-right: -4px;
  }
  .sort {
    flex: none;
    max-width: 120px;
    font-size: 12px;
    color: var(--color-text-muted);
  }
  @media (pointer: coarse) {
    .search {
      height: 44px;
    }
    .search .icon-btn {
      width: 44px;
      height: 42px;
      margin-right: calc(-1 * var(--space-2));
    }
    .sort {
      font-size: 14px;
    }
  }
  ul {
    flex: 1;
    overflow-y: auto;
    margin: 0;
    padding: 0 var(--space-2);
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .row {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 2px;
    padding: var(--space-2);
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    text-align: left;
  }
  @media (hover: hover) {
    .row:hover {
      background: var(--color-hover);
    }
  }
  .row.active {
    background: var(--color-selection);
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .top b {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .size {
    flex-shrink: 0;
  }
  .meta {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .meta .note {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta :last-child {
    flex: none;
  }
  footer {
    padding: var(--space-2) var(--space-4);
    /* at this size the UI font's space nearly vanishes after digits */
    word-spacing: 0.15em;
    border-top: 1px solid var(--color-border);
  }
  .empty {
    padding: var(--space-4);
  }
</style>
