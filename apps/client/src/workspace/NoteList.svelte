<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Note list with search, sort and new note. In lite layout the title is a
     scope picker, since there is no sidebar. -->
<script lang="ts">
  import { hour12 } from '../lib/clock.svelte'
  import { sharing } from '../lib/sharing.svelte'
  import { sync } from '../lib/sync.svelte'
  import SyncIcon from './SyncIcon.svelte'
  import { backend, MATCH_END, MATCH_START, type NoteSort, type Scope } from '../lib/backend'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { pullDown, swipeRow } from '../lib/gestures'
  import { thud } from '../lib/haptics'
  import { startNoteDrag } from '../lib/noteDrag'
  import { moveToItems } from '../lib/noteMenu'
  import { overlay, SEPARATOR } from '../lib/overlay.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import { workspace } from '../lib/workspace.svelte'
  import VirtualList from './VirtualList.svelte'

  let {
    narrow,
    searchFocus,
    onsettings,
  }: { narrow: boolean; searchFocus: number; onsettings: (section?: 'sync') => void } = $props()

  let searchInput: HTMLInputElement | undefined = $state()
  let fileInput: HTMLInputElement | undefined = $state()

  async function importPicked(input: HTMLInputElement) {
    await workspace.importFiles([...(input.files ?? [])])
    // lets the same file be picked again
    input.value = ''
  }
  $effect(() => {
    if (searchFocus > 0) searchInput?.focus()
  })

  const trash = $derived(workspace.scope.kind === 'trash')

  const title = $derived.by(() => {
    const scope = workspace.scope
    if (scope.kind === 'folder') return workspace.folderName(scope.id) || t('sidebar.allNotes')
    if (scope.kind === 'tag') return `#${workspace.tagName(scope.id)}`
    if (scope.kind === 'trash') return t('sidebar.trash')
    return t('sidebar.allNotes')
  })

  // lite layout: one select replaces the sidebar
  const scopeValue = $derived(
    workspace.scope.kind === 'folder' || workspace.scope.kind === 'tag'
      ? `${workspace.scope.kind}:${workspace.scope.id}`
      : workspace.scope.kind,
  )
  function pickScope(value: string) {
    const [kind, id] = value.split(':') as [string, string]
    const scope: Scope =
      kind === 'folder'
        ? { kind: 'folder', id }
        : kind === 'tag'
          ? { kind: 'tag', id }
          : kind === 'trash'
            ? { kind: 'trash' }
            : { kind: 'all' }
    void workspace.setScope(scope)
  }

  /** The notes as listed: the open one first when the setting says so. */
  const listed = $derived.by(() => {
    const open = workspace.current?.id
    const first = prefs.openNoteFirst ? workspace.notes.find((n) => n.id === open) : undefined
    return first ? [first, ...workspace.notes.filter((n) => n !== first)] : workspace.notes
  })

  async function changeSort(sort: NoteSort) {
    prefs.sort = sort
    await workspace.refreshNotes()
  }

  function when(at: number): string {
    const date = new Date(at)
    const now = new Date()
    const locale = currentLocale()
    if (date.toDateString() === now.toDateString())
      return date.toLocaleTimeString(locale, {
        hour: '2-digit',
        minute: '2-digit',
        hour12: hour12(),
      })
    if (now.getTime() - at < 6 * 86_400_000)
      return date.toLocaleDateString(locale, { weekday: 'short' })
    return date.toLocaleDateString(locale, { day: 'numeric', month: 'short' })
  }

  /** Splits a search snippet into plain and matched parts. */
  function parts(snippet: string) {
    return snippet.split(MATCH_START).flatMap((chunk, i) => {
      if (i === 0) return [{ text: chunk, hit: false }]
      const [hit = '', rest = ''] = chunk.split(MATCH_END)
      return [
        { text: hit, hit: true },
        { text: rest, hit: false },
      ]
    })
  }

  async function emptyTrash() {
    const yes = await overlay.ask(t('list.confirmEmptyTrash'), {
      confirmLabel: t('list.emptyTrash'),
      danger: true,
    })
    if (!yes) return
    await backend.emptyTrash()
    await workspace.refreshNotes()
  }

  async function purge(id: string) {
    const yes = await overlay.ask(t('list.confirmPurge'), {
      confirmLabel: t('list.purge'),
      danger: true,
    })
    if (yes) await workspace.purge(id)
  }

  /** The row whose swipe actions are showing, one at a time. */
  let swiped: string | null = $state(null)
  let closingTap = false
  /** Three actions of 56 px, a comfortable touch size. */
  const REVEAL = 168

  // a touch anywhere outside the open row puts it back
  $effect(() => {
    if (!swiped) return
    const id = swiped
    const close = (event: PointerEvent) => {
      const row = (event.target as Element).closest('.swipe')
      if (row?.querySelector(`[data-note="${id}"]`)) return
      swiped = null
      // the tap only closes, it does not also open the row it landed on
      closingTap = row !== null
    }
    window.addEventListener('pointerdown', close, true)
    return () => window.removeEventListener('pointerdown', close, true)
  })

  function swipedAction(run: () => void) {
    swiped = null
    run()
  }

  function moveFromSwipe(event: MouseEvent, note: { id: string; folderId: string | null }) {
    overlay.openMenuBelow(event.currentTarget as HTMLElement, moveToItems(note.id, note.folderId))
    swiped = null
  }

  function noteMenu(
    event: MouseEvent,
    note: { id: string; pinned: boolean; folderId: string | null; spaceId: string },
  ) {
    if (trash) {
      return overlay.openMenu(event, [
        { label: t('list.restore'), icon: 'restore', run: () => void workspace.restore(note.id) },
        { label: t('list.purge'), icon: 'trash', danger: true, run: () => void purge(note.id) },
      ])
    }
    // shared to read only: open is all there is
    if (sharing.readOnly(note.spaceId))
      return overlay.openMenu(event, [
        { label: t('menu.open'), run: () => void workspace.open(note.id) },
      ])
    overlay.openMenu(event, [
      { label: t('menu.open'), run: () => void workspace.open(note.id) },
      {
        label: note.pinned ? t('note.unpin') : t('note.pin'),
        icon: 'pin',
        run: () => void workspace.togglePin(note.id, !note.pinned),
      },
      { label: t('note.moveTo'), icon: 'move', items: moveToItems(note.id, note.folderId) },
      SEPARATOR,
      {
        label: t('note.trash'),
        icon: 'trash',
        danger: true,
        run: () => void workspace.trash(note.id),
      },
    ])
  }
</script>

<section
  class="list"
  class:narrow
  aria-label={t('list.label')}
  style:width={narrow ? undefined : `${prefs.listWidth}px`}
>
  <header>
    {#if prefs.layout === 'lite' || narrow}
      <h1 class="visually-hidden">{title}</h1>
      <select
        class="scope"
        aria-label={t('list.scopeLabel')}
        value={scopeValue}
        onchange={(e) => pickScope(e.currentTarget.value)}
      >
        <option value="all">{t('sidebar.allNotes')}</option>
        {#each workspace.folders as folder (folder.id)}
          <option value="folder:{folder.id}">{folder.name}</option>
        {/each}
        {#each workspace.tags as tag (tag.id)}
          <option value="tag:{tag.id}">#{tag.name}</option>
        {/each}
        <option value="trash">{t('sidebar.trash')}</option>
      </select>
    {:else}
      <h1>{title}</h1>
      {#if workspace.scope.kind === 'folder' || workspace.scope.kind === 'tag'}
        <button
          class="icon-btn clear"
          aria-label={t('list.showAll')}
          title={t('list.showAll')}
          onclick={() => workspace.setScope({ kind: 'all' })}
        >
          <Icon name="close" size={14} />
        </button>
      {/if}
    {/if}
    <div class="actions">
      {#if (prefs.layout === 'lite' || narrow) && sync.status.state !== 'off'}
        <button
          class="icon-btn"
          aria-label={t(`sync.state.${sync.status.state}`)}
          title={t(`sync.state.${sync.status.state}`)}
          onclick={() => onsettings('sync')}
        >
          <SyncIcon state={sync.status.state} size={18} />
        </button>
      {/if}
      {#if prefs.layout === 'lite' || narrow}
        <button class="icon-btn" aria-label={t('sidebar.settings')} onclick={() => onsettings()}>
          <Icon name="settings" />
        </button>
      {/if}
      {#if trash}
        <button class="btn danger" onclick={emptyTrash}>{t('list.emptyTrash')}</button>
      {:else}
        <button class="icon-btn" aria-label={t('list.import')} onclick={() => fileInput?.click()}>
          <Icon name="import" />
        </button>
        <input
          bind:this={fileInput}
          type="file"
          accept=".md,.markdown,.mdown,.mkd,text/markdown"
          multiple
          hidden
          onchange={(e) => importPicked(e.currentTarget)}
        />
        <button
          class="icon-btn new"
          aria-label={t('list.newNote')}
          onclick={() => workspace.create()}
        >
          <Icon name="newNote" />
        </button>
      {/if}
    </div>
  </header>

  <div class="tools">
    <label class="search">
      <Icon name="search" size={16} />
      <span class="visually-hidden">{t('list.search')}</span>
      <input
        bind:this={searchInput}
        type="search"
        placeholder={t('list.search')}
        value={workspace.query}
        oninput={(e) => workspace.search(e.currentTarget.value)}
      />
    </label>
    <select
      class="sort"
      aria-label={t('list.sort')}
      value={prefs.sort}
      onchange={(e) => changeSort(e.currentTarget.value as NoteSort)}
    >
      <option value="updatedDesc">{t('list.sortUpdated')}</option>
      <option value="createdDesc">{t('list.sortCreated')}</option>
      <option value="titleAsc">{t('list.sortTitle')}</option>
    </select>
  </div>

  <div class="pull" use:pullDown={{ scroller: '.rows', onPull: () => searchInput?.focus() }}>
    <p class="pull-hint" aria-hidden="true">{t('list.pullToSearch')}</p>
    {#if workspace.hits}
      <VirtualList items={workspace.hits} key={(hit) => hit.noteId} label={t('list.label')}>
        {#snippet row(hit)}
          <button
            class="row"
            class:selected={workspace.current?.id === hit.noteId}
            onclick={() => workspace.open(hit.noteId)}
          >
            <span class="title">{hit.title || t('note.untitled')}</span>
            <span class="snippet">
              {#each parts(hit.snippet) as part, i (i)}{#if part.hit}<mark>{part.text}</mark
                  >{:else}{part.text}{/if}{/each}
            </span>
          </button>
        {/snippet}
        {#snippet empty()}
          <li class="empty">{t('list.noResults')}</li>
        {/snippet}
      </VirtualList>
    {:else}
      <VirtualList items={listed} key={(note) => note.id} label={t('list.label')}>
        {#snippet row(note)}
          <div
            class="swipe"
            use:swipeRow={{
              disabled: trash,
              revealWidth: REVEAL,
              open: swiped === note.id,
              onOpen: () => (swiped = note.id),
              onClose: () => swiped === note.id && (swiped = null),
              onFull: () => {
                thud()
                swipedAction(() => void workspace.trash(note.id))
              },
              onRight: () => {
                thud()
                swipedAction(() => void workspace.togglePin(note.id, !note.pinned))
              },
            }}
          >
            {#if !trash}
              <div class="swipe-pin" aria-hidden="true"><Icon name="pin" /></div>
              <div class="swipe-actions">
                <button
                  class="pin"
                  tabindex={swiped === note.id ? 0 : -1}
                  onclick={() =>
                    swipedAction(() => void workspace.togglePin(note.id, !note.pinned))}
                >
                  <Icon name="pin" />{note.pinned ? t('note.unpin') : t('note.pin')}
                </button>
                <button
                  class="move"
                  tabindex={swiped === note.id ? 0 : -1}
                  onclick={(e) => moveFromSwipe(e, note)}
                >
                  <Icon name="move" />{t('note.moveTo')}
                </button>
                <button
                  class="trash"
                  tabindex={swiped === note.id ? 0 : -1}
                  onclick={() => swipedAction(() => void workspace.trash(note.id))}
                >
                  <Icon name="trash" />{t('list.swipeTrash')}
                </button>
              </div>
            {/if}
            <button
              class="row swipe-content"
              data-note={note.id}
              class:selected={workspace.current?.id === note.id}
              aria-current={workspace.current?.id === note.id ? 'true' : undefined}
              onclick={() => {
                if (closingTap) closingTap = false
                else if (swiped) swiped = null
                else void workspace.open(note.id)
              }}
              oncontextmenu={(e) => noteMenu(e, note)}
              draggable={!trash}
              ondragstart={(e) => startNoteDrag(e, note.id)}
            >
              <span class="title">
                {#if note.pinned}<Icon name="pin" size={13} label={t('note.pinned')} />{/if}
                {#if workspace.conflicted.includes(note.id)}<span class="conflicted"
                    ><Icon name="syncError" size={13} label={t('conflict.conflicted')} /></span
                  >{/if}
                <span class="name">{note.title || t('note.untitled')}</span>
              </span>
              <span class="snippet">{note.snippet}</span>
              <span class="date">{when(note.updatedAt)}</span>
            </button>
          </div>
          {#if trash}
            <div class="row-actions">
              <button class="btn" onclick={() => workspace.restore(note.id)}
                >{t('list.restore')}</button
              >
              <button class="btn danger" onclick={() => purge(note.id)}>
                {t('list.purge')}
              </button>
            </div>
          {/if}
        {/snippet}
        {#snippet empty()}
          <li class="empty">{trash ? t('list.trashEmpty') : t('list.empty')}</li>
        {/snippet}
      </VirtualList>
    {/if}
  </div>

  <footer class="muted">
    {t('list.count', { count: workspace.hits?.length ?? workspace.notes.length })}
  </footer>
</section>

<style>
  .list {
    flex: none;
    display: flex;
    flex-direction: column;
    background: var(--color-bg);
    border-right: 1px solid var(--color-border);
    min-height: 0;
  }
  .list.narrow {
    width: 100%;
    border-right: 0;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-3) var(--space-2) var(--space-4);
  }
  h1 {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 20px;
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope {
    min-width: 0;
    max-width: 70%;
    border: 0;
    background: transparent;
    font-size: 18px;
    font-weight: 650;
  }
  .actions {
    display: flex;
    gap: var(--space-1);
  }
  .new {
    color: var(--color-accent-text);
  }
  .tools {
    display: flex;
    gap: var(--space-2);
    padding: 0 var(--space-3) var(--space-2);
  }
  .search {
    flex: 1;
    /* without this the input keeps its default width and pushes the sort menu out */
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 32px;
    padding: 0 var(--space-2);
    border: 1px solid var(--color-border);
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
  @media (pointer: coarse) {
    .search {
      height: 44px;
    }
    .sort {
      font-size: 14px;
    }
  }
  .sort {
    flex: none;
    max-width: 120px;
    padding: 0 var(--space-1);
    font-size: 12px;
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface);
  }
  .row {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 2px var(--space-2);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 0;
    border-radius: var(--radius-control);
    background: var(--color-bg);
    text-align: left;
  }
  @media (hover: hover) {
    .row:hover {
      background: var(--color-surface);
    }
  }
  .swipe {
    position: relative;
    overflow: hidden;
    border-radius: var(--radius-control);
    /* sideways moves reach the swipe, up and down still scroll */
    touch-action: pan-y;
  }
  .swipe-content {
    position: relative;
    /* the wrapper rounds and clips both layers, a rounded row alone let the
       pin colour behind it show at the corners */
    border-radius: 0;
  }
  .swipe-pin,
  .swipe-actions {
    position: absolute;
    inset: 0;
    display: flex;
  }
  .swipe:not(:global(.revealed)) > .swipe-pin,
  .swipe:not(:global(.revealed)) > .swipe-actions {
    visibility: hidden;
  }
  .swipe-pin {
    align-items: center;
    padding-left: var(--space-5);
    background: var(--color-accent);
    color: var(--color-accent-on);
  }
  .swipe-actions {
    justify-content: flex-end;
  }
  .swipe-actions button {
    touch-action: pan-y;
  }
  .swipe-actions button {
    width: 56px;
    display: grid;
    place-items: center;
    align-content: center;
    gap: 2px;
    padding: 0 var(--space-1);
    border: 0;
    font-size: 11px;
    color: var(--color-text);
    background: var(--color-surface-2);
  }
  .swipe-actions .pin {
    background: var(--color-accent);
    color: var(--color-accent-on);
  }
  .swipe-actions .trash {
    background: var(--color-danger);
    color: var(--color-bg);
  }
  .pull {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .pull-hint {
    position: absolute;
    top: -28px;
    left: 0;
    right: 0;
    margin: 0;
    text-align: center;
    font-size: 12px;
    color: var(--color-text-muted);
    opacity: var(--pull, 0);
  }
  .row.selected {
    background: var(--color-selection);
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-weight: 600;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  /* a flex box cannot end its text with an ellipsis, its own child can */
  .title .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .title :global(.icon) {
    color: var(--color-accent-text);
  }
  .conflicted {
    display: inline-flex;
    color: var(--color-accent);
  }
  .snippet {
    grid-column: 1 / -1;
    color: var(--color-text-muted);
    font-size: 13px;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    /* always two lines, the virtual list needs rows of one height */
    height: 2lh;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .date {
    grid-row: 1;
    grid-column: 2;
    font-size: 12px;
    color: var(--color-text-muted);
  }
  mark {
    background: none;
    color: var(--color-accent-text);
    font-weight: 600;
  }
  .row-actions {
    display: flex;
    gap: var(--space-2);
    padding: 0 var(--space-3) var(--space-2);
  }
  .empty {
    padding: var(--space-6) var(--space-3);
    text-align: center;
    color: var(--color-text-muted);
  }
  footer {
    padding: var(--space-2) var(--space-4);
    font-size: 12px;
    border-top: 1px solid var(--color-border);
  }
</style>
