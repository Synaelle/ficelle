<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The open note: title, tags, editor, backlinks. -->
<script lang="ts">
  import { untrack } from 'svelte'
  import NoteFiles from './NoteFiles.svelte'
  import RawView from './RawView.svelte'
  import { hour12 } from '../lib/clock.svelte'
  import Editor from '../editor/Editor.svelte'
  import ConflictBanner from './ConflictBanner.svelte'
  import { onBack } from '../lib/back'
  import { modal } from '../lib/modal'
  import { backend, type NoteSummary, type NoteVersion } from '../lib/backend'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { tagItems } from '../lib/noteMenu'
  import { tagLabel } from '../lib/tagMark.svelte'
  import { overlay, SEPARATOR } from '../lib/overlay.svelte'
  import Knot from '@app/ui/Knot.svelte'
  import { prefs, shortcutFor } from '../lib/prefs.svelte'
  import { displayBinding } from '../lib/shortcuts'
  import type { MenuItem } from '../lib/overlay.svelte'
  import { workspace } from '../lib/workspace.svelte'

  let { narrow }: { narrow: boolean } = $props()

  const note = $derived(workspace.current)

  /** Keys the editor has not passed on yet: already not saved. */
  let typing = $state(false)
  const saving = $derived(workspace.dirty || typing)

  /** "Saved" shows for a moment after a save, then fades: it is the usual state. */
  let justSaved = $state(false)
  $effect(() => {
    if (saving) {
      justSaved = true
      return
    }
    const timer = setTimeout(() => (justSaved = false), 2000)
    return () => clearTimeout(timer)
  })

  /** The editor is made again when one of its settings changes or the note
   * syncs: the same note keeps its place, another one starts at the top. */
  const editorKey = $derived(
    note
      ? `${note.id}:${prefs.autoLists}:${prefs.emojiShortcodes}:${prefs.arrow}:${prefs.spellcheck}:${workspace.revision}`
      : '',
  )
  let scroller: HTMLDivElement | undefined = $state()
  /** The article's height while the new editor fills in, so the page does not
   * shrink under the place. */
  let hold = $state(0)
  let keyed = untrack(() => editorKey)
  $effect.pre(() => {
    const next = editorKey
    const box = scroller
    if (next === keyed) return
    const same = !!box && next.split(':')[0] === keyed.split(':')[0]
    keyed = next
    if (!same || !box) return
    const top = box.scrollTop
    const height = box.scrollHeight
    hold = box.querySelector('article')?.offsetHeight ?? 0
    const until = performance.now() + 2000
    // the editor makes itself after its files are read: wait for its height
    const keep = () => {
      box.scrollTop = top
      const filled = box.querySelector('.ProseMirror') && box.scrollHeight >= height - 1
      if (!filled && performance.now() < until) return void requestAnimationFrame(keep)
      hold = 0
      box.scrollTop = top
    }
    requestAnimationFrame(keep)
  })
  let backlinks: NoteSummary[] = $state([])
  let versions: NoteVersion[] | null = $state(null)
  const historyShown = $derived(versions !== null)
  $effect(() => {
    if (historyShown) return onBack(() => (versions = null))
  })
  let source = $state(false)
  /** The open editor, for the attachments list to add, mention and remove files. */
  let editorRef: Editor | undefined = $state()
  let sourceArea: HTMLTextAreaElement | undefined = $state()
  let tagDraft = $state('')
  let titleInput: HTMLInputElement | undefined = $state()
  /** Phones: no spelling popup on the title either, see the editor. */
  const PHONE = /android|iphone|ipad/i.test(navigator.userAgent)

  // a note just made: the caret in its title, ready to type
  $effect(() => {
    if (!note || !titleInput || workspace.fresh !== note.id) return
    workspace.fresh = null
    titleInput.focus()
  })

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
    const items: MenuItem[] = [
      { label: t('note.history'), icon: 'history', run: () => void showHistory() },
      {
        label: t('note.tags'),
        icon: 'tag',
        disabled: workspace.tags.length === 0,
        items: tagItems(id, note.tags),
      },
      SEPARATOR,
      {
        label: t('note.trash'),
        icon: 'trash',
        danger: true,
        run: () => void workspace.trash(id),
      },
    ]
    // a phone has no room for the tabs: Raw is a choice in this menu
    if (narrow)
      items.unshift(
        { label: t('note.raw'), icon: 'code', checked: source, run: () => (source = !source) },
        SEPARATOR,
      )
    overlay.openMenuBelow(button, items)
  }

  async function restore(version: NoteVersion) {
    const yes = await overlay.ask(t('note.restoreConfirm', { when: stamp(version.createdAt) }), {
      confirmLabel: t('note.restore'),
    })
    if (!yes) return
    await workspace.flush()
    workspace.replace(await backend.restoreVersion(version.id))
    versions = null
    await workspace.refreshNotes()
  }

  /** The start of a version's text, for telling entries apart: the
   * markdown marks and the title line left out. */
  function opening(version: NoteVersion): string {
    const lines = version.body.split('\n').map((line) => line.trim())
    if (lines[0]?.replace(/^#+\s*/, '') === version.title.trim()) lines.shift()
    const text = lines
      .join(' ')
      .replace(/[#>*_`~[\]()|-]+/g, ' ')
      .replace(/\s+/g, ' ')
      .trim()
    return text.length > 120 ? `${text.slice(0, 120)}…` : text
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
      <span class="where"></span>
      <span
        class="saved muted"
        class:bad={workspace.unsaved}
        class:quiet={!saving && !justSaved}
        aria-live="polite"
        >{workspace.unsaved
          ? t('note.notSaved')
          : saving
            ? t('note.saving')
            : t('note.saved')}</span
      >
      <button
        class="icon-btn"
        aria-label={note.pinned ? t('note.unpin') : t('note.pin')}
        aria-pressed={note.pinned}
        onclick={() => note && workspace.togglePin(note.id, !note.pinned)}
      >
        <Icon name="pin" />
      </button>
      <button
        class="icon-btn"
        aria-label={t('note.more')}
        aria-haspopup="menu"
        onclick={(e) => noteMenu(e.currentTarget)}
      >
        <Icon name="more" />
      </button>
    </header>

    <div class="scroll" bind:this={scroller} style:overflow-anchor={hold ? 'none' : null}>
      <article style:min-height={hold ? `${hold}px` : null}>
        <ConflictBanner {narrow} />
        <input
          bind:this={titleInput}
          class="title"
          spellcheck={prefs.spellcheck && !PHONE}
          placeholder={t('note.untitled')}
          aria-label={t('note.title')}
          value={note.title}
          oninput={(e) => workspace.edit({ title: e.currentTarget.value })}
          onkeydown={(e) => {
            // Enter goes on to the text, as in most note apps
            if (e.key !== 'Enter' || e.isComposing || e.shiftKey) return
            e.preventDefault()
            if (source) sourceArea?.focus()
            else editorRef?.focusStart()
          }}
        />
        <div class="tags">
          {#each note.tags as tag (tag)}
            {@const color = workspace.tags.find((o) => o.name === tag)?.color}
            <span class="tag">
              {#if prefs.tagMark === 'folder'}<Icon name="folder" size={12} />{/if}
              {tagLabel(tag)}
              {#if color}<span class="tag-mark" data-color={color}></span>{/if}
              <button
                aria-label={t('note.removeTag', { tag })}
                onclick={() => note && workspace.setTags(note.tags.filter((x) => x !== tag))}
                ><Icon name="close" size={12} /></button
              >
            </span>
          {/each}
          <form onsubmit={addTag}>
            <input
              class="tag-input"
              placeholder={t('note.addTag')}
              aria-label={t('note.addTag')}
              list="tag-choices"
              bind:value={tagDraft}
            />
            <datalist id="tag-choices">
              {#each workspace.tags as tag (tag.id)}<option value={tag.name}></option>{/each}
            </datalist>
          </form>
        </div>

        <!-- the formatted note, or its Markdown to edit as text (on a phone,
             Raw in the note's menu) -->
        {#if !narrow}<div class="views" role="tablist" aria-label={t('note.view')}>
            {#each [false, true] as raw (raw)}
              <button
                class="view-tab"
                class:on={source === raw}
                role="tab"
                aria-selected={source === raw}
                tabindex={source === raw ? 0 : -1}
                onclick={() => (source = raw)}
                onkeydown={(e) => {
                  if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
                  e.preventDefault()
                  source = !source
                  const tabs = e.currentTarget.parentElement?.querySelectorAll('button')
                  tabs?.[source ? 1 : 0]?.focus()
                }}
              >
                <Icon name={raw ? 'code' : 'eye'} size={16} />
                {raw ? t('note.raw') : t('note.preview')}
              </button>
            {/each}
          </div>{/if}

        {#if source}
          <RawView
            bind:field={sourceArea}
            label={t('note.rawLabel')}
            value={note.body}
            oninput={(body) => workspace.edit({ body })}
          />
        {:else}
          {#key editorKey}
            <Editor
              bind:this={editorRef}
              noteId={note.id}
              markdown={note.body}
              onchange={(body) => ((typing = false), workspace.edit({ body }))}
              onpending={(pending) => (typing = pending)}
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

        <NoteFiles noteId={note.id} body={note.body} editor={source ? undefined : editorRef} />
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
                {#if version.reason !== 'edit'}
                  <span class="reason">{t(`note.version_${version.reason}`)}</span>
                {/if}
                {#if opening(version)}<span class="opening">{opening(version)}</span>{/if}
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
    <div class="placeholder">
      <Knot size={56} />
      <p class="muted">{t('note.none')}</p>
      <button class="btn" onclick={() => void workspace.create()}>
        <Icon name="newNote" size={16} />
        {t('list.newNote')}
        <kbd>{displayBinding(shortcutFor('newNote'))}</kbd>
      </button>
    </div>
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
    transition: opacity var(--dur-slow) var(--ease-out);
  }
  .saved.quiet {
    opacity: 0;
  }
  .saved.bad {
    color: var(--color-danger);
  }
  /* a phone sideways with its keyboard up leaves a strip: the text being
     typed gets it, back stays the system's gesture */
  @media (max-height: 240px) {
    .narrow header {
      display: none;
    }
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
    margin: var(--space-2) 0 var(--space-3);
  }
  .views {
    display: flex;
    gap: var(--space-1);
    margin-bottom: var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }
  .view-tab {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 36px;
    padding: 0 var(--space-3);
    margin-bottom: -1px;
    border: 0;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--color-text-muted);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
  }
  .view-tab.on {
    color: var(--color-text);
    border-bottom-color: var(--color-accent);
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 1px var(--space-2);
    border-radius: var(--radius-chip);
    background: var(--color-surface-2);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .tag button {
    display: inline-grid;
    place-items: center;
    /* 24px, the smallest target a pointer should have to hit */
    min-width: 24px;
    min-height: 24px;
    border: 0;
    background: none;
    color: inherit;
    padding: 0;
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
  /* no box around it: a line under it while the keyboard is in it */
  .tag-input:focus-visible {
    box-shadow: inset 0 -2px 0 var(--color-accent);
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
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    opacity: 0.9;
  }
  .placeholder p {
    margin: 0;
  }
  .placeholder .btn {
    width: auto;
  }
  .placeholder kbd {
    color: var(--color-text-muted);
    font-size: 11px;
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
  .opening {
    font-size: 12px;
    color: var(--color-text-muted);
    overflow-wrap: anywhere;
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
