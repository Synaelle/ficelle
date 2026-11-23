<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Note list with search, sort and new note. On a phone the title is a
     scope picker, since there is no sidebar. -->
<script lang="ts">
  import { untrack } from 'svelte'
  import type { Action } from 'svelte/action'
  import { hour12 } from '../lib/clock.svelte'
  import { localSync } from '../lib/localSync.svelte'
  import { sync } from '../lib/sync.svelte'
  import SyncIcon from './SyncIcon.svelte'
  import { backend, MATCH_END, MATCH_START, type NoteSort } from '../lib/backend'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { pullDown, swipeRow } from '../lib/gestures'
  import { thud } from '../lib/haptics'
  import { startNoteDrag } from '../lib/noteDrag'
  import { tagItems } from '../lib/noteMenu'
  import { tagLabel, tagLeaf, tagParent, tagShort } from '../lib/tagMark.svelte'
  import { overlay, SEPARATOR } from '../lib/overlay.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import { UNFILED, workspace } from '../lib/workspace.svelte'
  import { noteWindowsSupported } from '../lib/windows'
  import { PHONE, WEB } from '../lib/platform'
  import UpdateMark from '../lib/UpdateMark.svelte'
  import TagSheet from './TagSheet.svelte'
  import VirtualList from './VirtualList.svelte'

  let {
    narrow,
    width,
    searchFocus,
    onsettings,
  }: {
    narrow: boolean
    width: number
    searchFocus: number
    onsettings: (section?: 'sync' | 'updates') => void
  } = $props()

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

  let rows: { focusStop: () => void } | undefined = $state()
  // a note sent to the trash takes its menu or row with it, and the focus
  // would fall to the page: the row now in its place takes it
  $effect(() => {
    if (workspace.trashed === 0) return
    const lost = !document.activeElement || document.activeElement === document.body
    if (lost) untrack(() => rows?.focusStop())
  })

  const trash = $derived(workspace.scope.kind === 'trash')
  /** A search looks through every note, whatever the list showed: the tag
   * and its sort make way for it. */
  const searching = $derived(workspace.hits !== null)

  /** The tag shown, with the tags it holds. */
  const shownTag = $derived.by(() => {
    const scope = workspace.scope
    return scope.kind === 'tag' ? workspace.tags.find((tag) => tag.id === scope.id) : undefined
  })
  const holdsTags = $derived(
    !!shownTag &&
      workspace.tags.some(
        (tag) => tagParent(tag.name).toLowerCase() === shownTag.name.toLowerCase(),
      ),
  )
  /** The levels above the tag shown, dimmed before its name. */
  const titleParent = $derived.by(() => {
    const parent = shownTag ? tagParent(shownTag.name) : ''
    return parent ? `${tagLabel(parent)} › ` : ''
  })

  const title = $derived.by(() => {
    const scope = workspace.scope
    if (shownTag) return tagLeaf(shownTag.name)
    if (scope.kind === 'untagged') return t('sidebar.untagged')
    if (scope.kind === 'trash') return t('sidebar.trash')
    return t('sidebar.allNotes')
  })

  /** On a phone the title opens the tags, in place of the sidebar. */
  let picking = $state(false)

  /** The notes as listed: the open one first when the setting says so. */
  const listed = $derived.by(() => {
    const open = workspace.current?.id
    const first = prefs.openNoteFirst ? workspace.notes.find((n) => n.id === open) : undefined
    return first ? [first, ...workspace.notes.filter((n) => n !== first)] : workspace.notes
  })
  /** Some note listed has tags: every row gets their line. */
  const withTags = $derived(listed.some((note) => note.tags.length > 0))

  /** A note's tags as its chips name them, in the order they read. */
  function chipsOf(tags: string[]) {
    return tags
      .map((tag) => ({
        tag,
        label: tagShort(tag, tags),
        color: workspace.tags.find((o) => o.name === tag)?.color,
      }))
      .sort((a, b) => a.label.localeCompare(b.label, currentLocale(), { sensitivity: 'base' }))
  }

  /** Keeps the chips that fit on the row's line and counts the others in a
   * last chip, "+2". The first chip, when alone and too long, ends in an
   * ellipsis instead. Its value, the tags as shown, only says when to fit
   * again. */
  const fitChips: Action<HTMLElement, string> = (box) => {
    const fit = () => {
      const chips = [...box.querySelectorAll<HTMLElement>('.chip:not(.more)')]
      const more = box.querySelector('.more') as HTMLElement
      for (const chip of chips) chip.hidden = false
      more.hidden = true
      const gap = parseFloat(getComputedStyle(box).columnGap)
      // as wide as their whole name, the first one may be shrunk already
      const widths = chips.map((chip) => {
        const label = chip.lastElementChild as HTMLElement
        return chip.offsetWidth + label.scrollWidth - label.clientWidth
      })
      const span = (count: number) =>
        widths.slice(0, count).reduce((sum, width) => sum + width + gap, -gap)
      if (chips.length < 2 || span(chips.length) <= box.clientWidth) return
      more.hidden = false
      let shown = chips.length - 1
      more.textContent = '+1'
      while (shown > 1 && span(shown) + gap + more.offsetWidth > box.clientWidth) {
        shown -= 1
        more.textContent = `+${chips.length - shown}`
      }
      for (const chip of chips.slice(shown)) chip.hidden = true
    }
    const resized = new ResizeObserver(fit)
    resized.observe(box)
    return { update: fit, destroy: () => resized.disconnect() }
  }

  async function changeSort(sort: NoteSort) {
    prefs.sort = sort
    await workspace.refreshNotes()
  }

  function when(at: number): string {
    const date = new Date(at)
    const now = new Date()
    const locale = currentLocale()
    if (date.toDateString() === now.toDateString())
      // 13:42 and 09:05, but 1:42 PM: a 12-hour clock writes no leading zero
      return date.toLocaleTimeString(locale, {
        hour: hour12() ? 'numeric' : '2-digit',
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
  /** Two actions of 56 px, a comfortable touch size. */
  const REVEAL = 112

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

  function tagsFromSwipe(event: MouseEvent, note: { id: string; tags: string[] }) {
    overlay.openMenuBelow(event.currentTarget as HTMLElement, tagItems(note.id, note.tags))
    swiped = null
  }

  /** The row a menu was opened on, marked while the menu stays open. */
  let menuFor: string | null = $state(null)
  $effect(() => {
    if (!overlay.menu) menuFor = null
  })

  function noteMenu(
    event: MouseEvent,
    note: { id: string; title: string; pinned: boolean; tags: string[] },
  ) {
    const name = note.title || t('note.untitled')
    menuFor = note.id
    if (trash) {
      return overlay.openMenu(
        event,
        [
          { label: t('list.restore'), icon: 'restore', run: () => void workspace.restore(note.id) },
          { label: t('list.purge'), icon: 'trash', danger: true, run: () => void purge(note.id) },
        ],
        name,
      )
    }
    overlay.openMenu(
      event,
      [
        // a phone opens a note with a tap, the menu only repeats it
        ...(narrow
          ? []
          : [
              {
                label: t('menu.open'),
                icon: 'markdown' as const,
                run: () => void workspace.open(note.id),
              },
            ]),
        ...(noteWindowsSupported
          ? [
              {
                label: t('menu.openInWindow'),
                icon: 'window' as const,
                run: () => void workspace.openInWindow(note.id),
              },
            ]
          : []),
        {
          label: note.pinned ? t('note.unpin') : t('note.pin'),
          icon: 'pin',
          run: () => void workspace.togglePin(note.id, !note.pinned),
        },
        {
          label: t('note.tags'),
          icon: 'tag',
          disabled: workspace.tags.length === 0,
          items: tagItems(note.id, note.tags),
        },
        SEPARATOR,
        {
          label: t('note.trash'),
          icon: 'trash',
          danger: true,
          run: () => void workspace.trash(note.id),
        },
      ],
      name,
    )
  }
</script>

<section
  class="list"
  class:narrow
  aria-label={t('list.label')}
  style:width={narrow ? undefined : `${width}px`}
>
  <header>
    {#if narrow}
      <h1 class="visually-hidden">{title}</h1>
      <button
        class="scope"
        aria-haspopup="dialog"
        aria-expanded={picking}
        onclick={() => (picking = true)}
        ><span class="dim">{titleParent}</span><span class="leaf">{title}</span><Icon
          name="down"
          size={18}
        /></button
      >
    {:else if searching}
      <h1>{t('list.found')}</h1>
    {:else}
      <h1><span class="dim">{titleParent}</span>{title}</h1>
      {#if workspace.scope.kind === 'tag'}
        <button
          class="icon-btn clear"
          aria-label={t('list.clearFilter')}
          title={t('list.clearFilter')}
          onclick={() => workspace.setScope(UNFILED)}
        >
          <Icon name="close" size={14} />
        </button>
      {/if}
    {/if}
    <div class="actions">
      <!-- sync shows only when it needs a look, as an icon: a line of text
           changing with every round would flicker; importing is in Settings -->
      {#if ['signedOut', 'offline', 'error'].includes(sync.status.state)}
        <button
          class="icon-btn"
          aria-label={t(`sync.state.${sync.status.state}`)}
          title={t(`sync.state.${sync.status.state}`)}
          onclick={() => onsettings('sync')}
        >
          <SyncIcon state={sync.status.state} size={18} />
        </button>
      {:else if localSync.summary?.state === 'error'}
        {@const line = t(`local.status.${localSync.summary.key}`, { name: localSync.summary.name })}
        <button class="icon-btn" aria-label={line} title={line} onclick={() => onsettings('sync')}>
          <SyncIcon state="error" size={18} />
        </button>
      {/if}
      {#if PHONE}
        <UpdateMark onopen={() => onsettings('updates')} />
      {/if}
      {#if narrow}
        <button class="icon-btn" aria-label={t('sidebar.settings')} onclick={() => onsettings()}>
          <Icon name="settings" />
        </button>
      {/if}
      {#if trash}
        <!-- only when there is something to empty, an icon on narrow screens -->
        {#if workspace.notes.length > 0}
          {#if narrow}
            <button
              class="icon-btn danger"
              aria-label={t('list.emptyTrash')}
              title={t('list.emptyTrash')}
              onclick={emptyTrash}><Icon name="trash" /></button
            >
          {:else}
            <button class="btn danger" onclick={emptyTrash}>{t('list.emptyTrash')}</button>
          {/if}
        {/if}
      {:else}
        {#if !narrow}
          <!-- a Markdown file, not the tray of import and export: the other
               importers are in Settings -->
          <button
            class="icon-btn"
            aria-label={t('list.import')}
            title={t('list.import')}
            onclick={() => fileInput?.click()}
          >
            <Icon name="markdown" />
          </button>
          <input
            bind:this={fileInput}
            type="file"
            accept=".md,.markdown,.mdown,.mkd,text/markdown"
            multiple
            hidden
            onchange={(e) => importPicked(e.currentTarget)}
          />
        {/if}
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
  {#if holdsTags && !narrow && !searching}
    <p class="inner">{t('list.withInside', { count: workspace.notes.length })}</p>
  {/if}
  {#if trash && !searching}
    <p class="inner">{t('list.trashKept')}</p>
  {/if}

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
      {#if workspace.query}
        <button
          type="button"
          class="icon-btn"
          aria-label={t('list.clearSearch')}
          title={t('list.clearSearch')}
          onclick={() => {
            void workspace.search('')
            searchInput?.focus()
          }}
        >
          <Icon name="close" size={14} />
        </button>
      {/if}
    </label>
    {#if !searching}
      <select
        class="sort"
        aria-label={t('list.sort')}
        value={prefs.sort}
        onchange={(e) => changeSort(e.currentTarget.value as NoteSort)}
      >
        <option value="folderAsc">{t('list.sortFolder')}</option>
        <option value="updatedDesc">{t('list.sortUpdated')}</option>
        <option value="createdDesc">{t('list.sortCreated')}</option>
        <option value="titleAsc">{t('list.sortTitle')}</option>
      </select>
    {/if}
  </div>

  <div class="pull" use:pullDown={{ scroller: '.rows', onPull: () => searchInput?.focus() }}>
    <p class="pull-hint" aria-hidden="true">{t('list.pullToSearch')}</p>
    {#if workspace.hits}
      <VirtualList
        items={workspace.hits}
        key={(hit) => hit.noteId}
        label={t('list.label')}
        active={workspace.current?.id}
      >
        {#snippet row(hit, tabbable)}
          <button
            class="row"
            tabindex={tabbable ? 0 : -1}
            class:selected={workspace.current?.id === hit.noteId}
            onclick={() => workspace.openFound(hit.noteId)}
          >
            <span class="title">{hit.title || t('note.untitled')}</span>
            <span class="snippet" class:blank={!hit.snippet.trim()}>
              {#if !hit.snippet.trim()}{t('list.noText')}{/if}
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
      <VirtualList
        bind:this={rows}
        items={listed}
        key={(note) => note.id}
        label={t('list.label')}
        active={workspace.current?.id}
      >
        {#snippet row(note, tabbable)}
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
              <!-- pinning is the swipe the other way -->
              <div class="swipe-actions">
                <button
                  class="move"
                  tabindex={swiped === note.id ? 0 : -1}
                  aria-label={t('note.tags')}
                  title={t('note.tags')}
                  onclick={(e) => tagsFromSwipe(e, note)}
                >
                  <Icon name="tag" />
                </button>
                <button
                  class="trash"
                  tabindex={swiped === note.id ? 0 : -1}
                  aria-label={t('list.swipeTrash')}
                  title={t('list.swipeTrash')}
                  onclick={() => swipedAction(() => void workspace.trash(note.id))}
                >
                  <Icon name="trash" />
                </button>
              </div>
            {/if}
            <button
              class="row swipe-content"
              tabindex={tabbable ? 0 : -1}
              data-note={note.id}
              class:selected={workspace.current?.id === note.id}
              class:pressed={menuFor === note.id}
              aria-current={workspace.current?.id === note.id ? 'true' : undefined}
              onclick={(e) => {
                if (closingTap) closingTap = false
                else if (swiped) swiped = null
                else if (noteWindowsSupported && (e.ctrlKey || e.metaKey))
                  void workspace.openInWindow(note.id)
                else void workspace.open(note.id)
              }}
              onauxclick={(e) => {
                if (noteWindowsSupported && e.button === 1) void workspace.openInWindow(note.id)
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
              <!-- an empty line would read as a gap between the title and the tags -->
              <span class="snippet" class:blank={!note.snippet.trim()}
                >{note.snippet.trim() ? note.snippet : t('list.noText')}</span
              >
              <!-- the list measures one row for all: when any note has tags,
                   every row keeps the line for them -->
              {#if withTags}
                <span class="chips" use:fitChips={note.tags.join('\n')}>
                  {#each chipsOf(note.tags) as chip (chip.tag)}
                    <span class="chip" title={tagLabel(chip.tag)}
                      >{#if chip.color}<span class="tag-mark" data-color={chip.color}
                        ></span>{/if}<span class="label">{chip.label}</span></span
                    >
                  {/each}
                  <span class="chip more" hidden></span>
                </span>
              {/if}
              <span class="date">{when(note.updatedAt)}</span>
            </button>
            {#if trash}
              <div class="row-actions">
                <button
                  class="icon-btn"
                  tabindex={tabbable ? 0 : -1}
                  aria-label={t('list.restore')}
                  title={t('list.restore')}
                  onclick={() => workspace.restore(note.id)}><Icon name="restore" /></button
                >
                <button
                  class="icon-btn purge"
                  tabindex={tabbable ? 0 : -1}
                  aria-label={t('list.purge')}
                  title={t('list.purge')}
                  onclick={() => purge(note.id)}><Icon name="trash" /></button
                >
              </div>
            {/if}
          </div>
        {/snippet}
        {#snippet empty()}
          <li class="empty">
            {trash
              ? t('list.trashEmpty')
              : workspace.scope.kind === 'untagged'
                ? t('list.untaggedEmpty')
                : t('list.empty')}
            {#if !trash}
              <button class="btn" onclick={() => void workspace.create()}>
                <Icon name="newNote" size={16} />
                {t('list.newNote')}
              </button>
            {/if}
            <!-- no note and no tag yet: a first start, where the notes are and
                 what else there is. The web version has its own welcome -->
            {#if !WEB && workspace.scope.kind === 'untagged' && workspace.tags.length === 0}
              <span>{t('list.firstStart')}</span>
            {/if}
          </li>
        {/snippet}
      </VirtualList>
    {/if}
  </div>

  <footer class="muted">
    {workspace.hits && workspace.moreHits
      ? t('list.countMore', { count: workspace.hits.length })
      : t('list.count', { count: workspace.hits?.length ?? workspace.notes.length })}
  </footer>
  {#if picking}
    <TagSheet close={() => (picking = false)} />
  {/if}
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
    font-size: var(--title-screen);
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope {
    min-width: 0;
    max-width: 70%;
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
  .scope .dim {
    flex: none;
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14px;
    font-weight: 500;
  }
  .scope .leaf {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope :global(svg) {
    flex: none;
    color: var(--color-text-muted);
  }
  .icon-btn.danger {
    color: var(--color-danger);
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
    padding: 0 var(--space-4) var(--space-2);
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
  /* the engine's own clear glyph, a close icon in its place */
  .search input::-webkit-search-cancel-button {
    display: none;
  }
  .search .icon-btn {
    width: 24px;
    height: 24px;
    margin-right: -4px;
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
  .sort {
    flex: none;
    max-width: 120px;
    font-size: 12px;
    color: var(--color-text-muted);
  }
  .row {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 2px var(--space-2);
    width: 100%;
    /* with the list's own inset, the text lines up with the title and the search */
    padding: var(--space-2);
    border: 0;
    border-radius: var(--radius-control);
    background: var(--color-bg);
    text-align: left;
  }
  @media (hover: hover) {
    .row:hover {
      background: var(--color-hover);
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
    /* it overlaps the search box above the list, which keeps every click */
    pointer-events: none;
  }
  .row.selected,
  .row.pressed {
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
  .chips {
    grid-column: 1 / -1;
    display: flex;
    gap: var(--space-1);
    min-height: 19px;
    margin-top: 2px;
    overflow: hidden;
  }
  .chip {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 5px;
    padding: 1px 6px;
    border-radius: var(--radius-chip);
    background: var(--color-surface-2);
    font-size: 12px;
    line-height: 17px;
    color: var(--color-text-muted);
    white-space: nowrap;
  }
  .chip[hidden] {
    display: none;
  }
  /* a tag name longer than the row, see fitChips */
  .chip:first-child {
    flex-shrink: 1;
    min-width: 0;
  }
  .chip .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .inner {
    margin: 0;
    padding: 0 var(--space-4) var(--space-2);
    font-size: 12px;
    color: var(--color-text-muted);
  }
  h1 .dim {
    font-weight: 400;
    color: var(--color-text-muted);
  }
  .snippet {
    grid-column: 1 / -1;
    color: var(--color-text-muted);
    font-size: 13px;
    /* one line, always there, the virtual list needs rows of one height */
    height: 1lh;
    white-space: nowrap;
    text-overflow: ellipsis;
    overflow: hidden;
  }
  .snippet.blank {
    font-style: italic;
  }
  /* smaller than the title, it shares its baseline rather than its middle */
  .title,
  .title .name,
  .date {
    align-self: baseline;
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
  /* in the Trash, two small buttons at the row's right, rows keep their height */
  .row-actions {
    position: absolute;
    top: 50%;
    right: var(--space-1);
    transform: translateY(-50%);
    display: flex;
  }
  .row-actions .purge {
    color: var(--color-danger);
  }
  .swipe:has(.row-actions) .row {
    padding-right: 96px;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-6) var(--space-3);
    text-align: center;
    color: var(--color-text-muted);
  }
  footer {
    padding: var(--space-2) var(--space-4);
    font-size: 12px;
    /* at this size the UI font's space nearly vanishes after digits */
    word-spacing: 0.15em;
    border-top: 1px solid var(--color-border);
  }
</style>
