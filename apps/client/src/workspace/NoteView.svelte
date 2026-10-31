<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The open note: title, tags, editor, backlinks. -->
<script lang="ts">
  import NoteFiles from './NoteFiles.svelte'
  import { hour12 } from '../lib/clock.svelte'
  import Editor from '../editor/Editor.svelte'
  import ConflictBanner from './ConflictBanner.svelte'
  import { onBack } from '../lib/back'
  import { modal } from '../lib/modal'
  import { backend, type NoteSummary, type NoteVersion } from '../lib/backend'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { moveToItems } from '../lib/noteMenu'
  import { overlay, SEPARATOR } from '../lib/overlay.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import { sharing } from '../lib/sharing.svelte'
  import { sync } from '../lib/sync.svelte'
  import type { MenuItem } from '../lib/overlay.svelte'
  import { workspace } from '../lib/workspace.svelte'

  let { narrow }: { narrow: boolean } = $props()

  const note = $derived(workspace.current)
  const readonly = $derived(sharing.readOnly(note?.spaceId))
  const others = $derived(note ? sync.others(note.spaceId, note.id) : [])
  const initials = (name: string) =>
    name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0]?.toUpperCase())
      .join('')
  let backlinks: NoteSummary[] = $state([])
  let versions: NoteVersion[] | null = $state(null)
  const historyShown = $derived(versions !== null)
  $effect(() => {
    if (historyShown) return onBack(() => (versions = null))
  })
  let source = $state(false)
  let tagDraft = $state('')

  $effect(() => {
    const id = note?.id
    source = false
    versions = null
    backlinks = []
    if (id) void backend.backlinks(id).then((list) => (backlinks = list))
  })

  function addTag(event: SubmitEvent) {
    event.preventDefault()
    if (!note || !tagDraft.trim()) return
    void workspace.setTags([...note.tags, tagDraft])
    tagDraft = ''
  }

  async function showHistory() {
    if (note) versions = await backend.listVersions(note.id)
  }

  function noteMenu(button: HTMLElement) {
    if (!note) return
    const id = note.id
    const name = note.title || t('note.untitled')
    const items: MenuItem[] = [
      {
        label: source ? t('note.rendered') : t('note.source'),
        icon: 'code',
        run: () => (source = !source),
      },
    ]
    if (sharing.available && note.spaceId === '')
      items.push({
        label: t('share.shareItem'),
        icon: 'share',
        run: () => sharing.open({ item: { kind: 'note', id }, name }),
      })
    const root = sharing.rootOf(id)
    if (root)
      items.push({
        label: t('share.sharing'),
        icon: 'share',
        run: () => sharing.open({ space: root.id, name }),
      })
    if (!readonly)
      items.push(
        { label: t('note.history'), icon: 'history', run: () => void showHistory() },
        { label: t('note.moveTo'), icon: 'move', items: moveToItems(id, note.folderId) },
        SEPARATOR,
        {
          label: t('note.trash'),
          icon: 'trash',
          danger: true,
          run: () => void workspace.trash(id),
        },
      )
    overlay.openMenuBelow(button, items)
  }

  async function restore(version: NoteVersion) {
    await workspace.flush()
    workspace.replace(await backend.restoreVersion(version.id))
    versions = null
    await workspace.refreshNotes()
  }

  const stamp = (at: number) =>
    new Date(at).toLocaleString(currentLocale(), {
      dateStyle: 'medium',
      timeStyle: 'short',
      hour12: hour12(),
    })
</script>

<section class="view" class:narrow aria-label={t('note.label')}>
  {#if note}
    <header>
      {#if narrow}
        <button class="icon-btn" aria-label={t('common.back')} onclick={() => workspace.open(null)}>
          <Icon name="back" />
        </button>
      {/if}
      <span class="where muted">{workspace.folderName(note.folderId)}</span>
      {#if others.length}
        <ul class="viewers" aria-label={t('presence.label')}>
          {#each others as viewer (viewer.name)}
            <li title={t('presence.here', { name: viewer.displayName || viewer.name })}>
              <span aria-hidden="true">{initials(viewer.displayName || viewer.name)}</span>
              <span class="visually-hidden"
                >{t('presence.here', { name: viewer.displayName || viewer.name })}</span
              >
            </li>
          {/each}
        </ul>
      {/if}
      <span class="saved muted" aria-live="polite"
        >{workspace.dirty ? t('note.saving') : t('note.saved')}</span
      >
      {#if !readonly}
        <button
          class="icon-btn"
          aria-label={note.pinned ? t('note.unpin') : t('note.pin')}
          aria-pressed={note.pinned}
          onclick={() => note && workspace.togglePin(note.id, !note.pinned)}
        >
          <Icon name="pin" />
        </button>
      {/if}
      <button
        class="icon-btn"
        aria-label={t('note.more')}
        aria-haspopup="menu"
        onclick={(e) => noteMenu(e.currentTarget)}
      >
        <Icon name="more" />
      </button>
    </header>

    <div class="scroll">
      <article>
        <ConflictBanner {narrow} />
        {#if readonly}
          <p class="readonly-banner" role="note">
            <Icon name="lock" size={16} />{t('share.readOnlyBanner', {
              owner: sharing.space(note.spaceId)?.ownerName ?? '',
            })}
          </p>
        {/if}
        <input
          class="title"
          placeholder={t('note.untitled')}
          aria-label={t('note.title')}
          {readonly}
          value={note.title}
          oninput={(e) => workspace.edit({ title: e.currentTarget.value })}
        />
        <div class="tags">
          {#each note.tags as tag (tag)}
            <span class="tag">
              #{tag}
              {#if !readonly}
                <button
                  aria-label={t('note.removeTag', { tag })}
                  onclick={() => note && workspace.setTags(note.tags.filter((x) => x !== tag))}
                  >×</button
                >
              {/if}
            </span>
          {/each}
          {#if !readonly}
            <form onsubmit={addTag}>
              <input class="tag-input" placeholder={t('note.addTag')} bind:value={tagDraft} />
            </form>
          {/if}
        </div>

        {#if source}
          <textarea
            class="source"
            spellcheck="false"
            {readonly}
            aria-label={t('note.source')}
            value={note.body}
            oninput={(e) => workspace.edit({ body: e.currentTarget.value })}></textarea>
        {:else}
          {#key `${note.id}:${prefs.autoLists}:${prefs.emojiShortcodes}:${workspace.revision}:${readonly}`}
            <Editor
              noteId={note.id}
              markdown={note.body}
              {readonly}
              onchange={(body) => workspace.edit({ body })}
              onopenlink={(title, heading) => workspace.openByTitle(title, heading)}
            />
          {/key}
        {/if}

        {#if backlinks.length}
          <aside class="backlinks">
            <h2>{t('note.backlinks', { count: backlinks.length })}</h2>
            <ul>
              {#each backlinks as link (link.id)}
                <li>
                  <button onclick={() => workspace.open(link.id)}
                    >{link.title || t('note.untitled')}</button
                  >
                </li>
              {/each}
            </ul>
          </aside>
        {/if}

        <NoteFiles noteId={note.id} body={note.body} />
      </article>
    </div>

    {#if versions}
      <div
        class="sheet"
        role="dialog"
        aria-label={t('note.history')}
        use:modal={{ onclose: () => (versions = null) }}
      >
        <header>
          <h2>{t('note.history')}</h2>
          <button class="icon-btn" aria-label={t('common.close')} onclick={() => (versions = null)}
            ><Icon name="close" /></button
          >
        </header>
        <ul>
          {#each versions as version (version.id)}
            <li>
              <div>
                <strong>{stamp(version.createdAt)}</strong>
                <span class="muted">{version.title || t('note.untitled')}</span>
                {#if version.reason === 'mineBeforeMerge' || version.reason === 'theirsBeforeMerge'}
                  <span class="reason">{t(`note.version_${version.reason}`)}</span>
                {/if}
              </div>
              <button class="btn" onclick={() => restore(version)}
                ><Icon name="restore" size={16} />{t('note.restore')}</button
              >
            </li>
          {:else}
            <li class="muted">{t('note.noHistory')}</li>
          {/each}
        </ul>
      </div>
    {/if}
  {:else}
    <div class="placeholder muted">{t('note.none')}</div>
  {/if}
</section>

<style>
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--color-surface);
    position: relative;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 48px;
    padding: 0 var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }
  .where {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .saved {
    font-size: 12px;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
  }
  article {
    max-width: 760px;
    margin: 0 auto;
    padding: var(--space-6) var(--space-6) 120px;
  }
  .narrow article {
    padding: var(--space-4) var(--space-4) 120px;
  }
  .title {
    width: 100%;
    border: 0;
    outline: 0;
    background: transparent;
    font-family: var(--font-editor);
    font-size: 30px;
    font-weight: 700;
    line-height: 1.2;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-2) 0 var(--space-5);
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 1px var(--space-2);
    border-radius: var(--radius-control);
    background: var(--color-surface-2);
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .tag button {
    border: 0;
    background: none;
    color: inherit;
    padding: 0 2px;
  }
  @media (pointer: coarse) {
    .tag {
      min-height: 36px;
      font-size: 14px;
    }
    .tag button {
      min-width: 32px;
      min-height: 36px;
    }
    .tag-input {
      min-height: 44px;
    }
  }
  .tag-input {
    width: 120px;
    border: 0;
    outline: 0;
    background: transparent;
    font-size: 13px;
    color: var(--color-text-muted);
  }
  .source {
    width: 100%;
    min-height: 60vh;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-bg);
    font-family: var(--font-mono);
    font-size: 13px;
    line-height: 1.6;
    resize: vertical;
  }
  .backlinks {
    margin-top: var(--space-7);
    padding-top: var(--space-4);
    border-top: 1px solid var(--color-border);
  }
  .backlinks h2 {
    margin: 0 0 var(--space-2);
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-muted);
  }
  .backlinks ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .backlinks button {
    padding: var(--space-1) 0;
    border: 0;
    background: none;
    color: var(--color-accent-text);
  }
  .placeholder {
    flex: 1;
    display: grid;
    place-items: center;
  }
  .viewers {
    display: flex;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .viewers li {
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    margin-left: -6px;
    border: 2px solid var(--color-bg);
    border-radius: 50%;
    background: var(--color-accent);
    color: var(--color-accent-on);
    font-size: 11px;
    font-weight: 600;
  }
  .readonly-banner {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0 0 var(--space-3);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface);
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .sheet {
    position: absolute;
    inset: 0 0 0 auto;
    /* above the editor toolbar, which is sticky with its own layer */
    z-index: 20;
    width: min(360px, 100%);
    display: flex;
    flex-direction: column;
    background: var(--color-bg);
    border-left: 1px solid var(--color-border);
  }
  /* phones: history covers the note, a strip of it beside is no use */
  .narrow .sheet {
    width: 100%;
    border-left: 0;
    /* over the formatting bar fixed at the bottom on phones */
    z-index: 35;
  }
  .reason {
    font-size: 12px;
    color: var(--color-accent);
  }
  .sheet h2 {
    flex: 1;
    margin: 0;
    font-size: 15px;
  }
  .sheet ul {
    margin: 0;
    padding: var(--space-2);
    list-style: none;
    overflow-y: auto;
  }
  .sheet li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }
  .sheet li div {
    display: grid;
  }
</style>
