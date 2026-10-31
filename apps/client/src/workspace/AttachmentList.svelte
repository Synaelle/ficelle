<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Every attachment, in place of the note list when Attachments is chosen
     in the sidebar: name, type, size, its note and when it was added, newest
     first, with a filter by name and a sort. A file opens its note. Notes in
     the trash are left out. -->
<script lang="ts">
  import { onDestroy } from 'svelte'
  import { backend, type NoteAttachment } from '../lib/backend'
  import { bytes } from '../lib/format'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { FILES_CHANGED } from '../lib/sync.svelte'
  import { workspace } from '../lib/workspace.svelte'

  let all: NoteAttachment[] = $state([])
  let filter = $state('')
  let sort = $state<'newest' | 'largest' | 'name'>('newest')

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
  const kind = (file: NoteAttachment) =>
    file.fileName.includes('.') ? file.fileName.split('.').pop()?.toUpperCase() : file.mimeType
  const day = (ms: number) =>
    new Intl.DateTimeFormat(currentLocale(), { dateStyle: 'medium' }).format(ms)
</script>

<section class="list" aria-label={t('files.allTitle')}>
  <header>
    <h1>{t('files.allTitle')}</h1>
    <span class="muted">{t('files.allCount', { count: all.length, size: bytes(total) })}</span>
  </header>
  <div class="tools">
    <input
      class="field"
      type="search"
      placeholder={t('files.filter')}
      aria-label={t('files.filter')}
      bind:value={filter}
    />
    <select class="field" bind:value={sort} aria-label={t('files.sort')}>
      <option value="newest">{t('files.sort_newest')}</option>
      <option value="largest">{t('files.sort_largest')}</option>
      <option value="name">{t('files.sort_name')}</option>
    </select>
  </div>
  {#if shown.length}
    <ul>
      {#each shown as file (file.id)}
        <li>
          <button
            class:active={workspace.current?.id === file.noteId}
            onclick={() => workspace.open(file.noteId)}
          >
            <span class="top">
              <Icon name={file.mimeType.startsWith('image/') ? 'image' : 'attachment'} size={14} />
              <b>{file.fileName}</b>
              <span class="muted size">{bytes(file.size)}</span>
            </span>
            <span class="muted"
              >{kind(file)} · {file.noteTitle || t('note.untitled')} · {day(file.createdAt)}</span
            >
          </button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="muted empty">{all.length ? t('files.noMatch') : t('files.none')}</p>
  {/if}
</section>

<style>
  .list {
    display: flex;
    flex-direction: column;
    min-width: 0;
    height: 100%;
    border-right: 1px solid var(--color-border);
  }
  header {
    display: grid;
    gap: 2px;
    padding: var(--space-4) var(--space-4) var(--space-2);
  }
  h1 {
    margin: 0;
    font-size: 18px;
  }
  .tools {
    display: flex;
    gap: var(--space-2);
    padding: 0 var(--space-4) var(--space-2);
  }
  .tools input {
    flex: 1;
    min-width: 0;
    width: auto;
  }
  .tools select {
    flex: none;
    width: auto;
  }
  ul {
    flex: 1;
    overflow-y: auto;
    margin: 0;
    padding: 0 var(--space-2);
    list-style: none;
  }
  li button {
    display: grid;
    gap: 2px;
    width: 100%;
    padding: var(--space-2);
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    text-align: left;
  }
  li button:hover {
    background: var(--color-surface);
  }
  li button.active {
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
  .empty {
    padding: var(--space-4);
  }
</style>
