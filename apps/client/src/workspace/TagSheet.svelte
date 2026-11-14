<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- On a phone, where the list's title leads: a sheet from the bottom with
     the notes with no tag, all notes, the tags as a tree that folds, and the
     trash. Typing finds a tag at any level and says where it lives. -->
<script lang="ts">
  import type { Scope, Tag } from '../lib/backend'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { modal } from '../lib/modal'
  import { prefs } from '../lib/prefs.svelte'
  import { tagLabel, tagLeaf, tagParent } from '../lib/tagMark.svelte'
  import { tagKey, UNFILED, workspace } from '../lib/workspace.svelte'

  let { close }: { close: () => void } = $props()

  let query = $state('')
  let sheet: HTMLDivElement | undefined = $state()
  /** The sheet's height before the search, kept while it shows fewer rows,
   * so the field does not move under the finger. */
  let openHeight = $state(0)

  const current = $derived(workspace.scope)
  const currentTag = $derived(
    current.kind === 'tag' ? workspace.tags.find((tag) => tag.id === current.id) : undefined,
  )
  const holds = (tag: Tag) =>
    workspace.tags.some((o) => tagKey(tagParent(o.name)) === tagKey(tag.name))
  const childrenOf = (parent: string) =>
    workspace.tags.filter((tag) => tagKey(tagParent(tag.name)) === tagKey(parent))

  /** Unfolded tags, by lowercased path: the levels above the tag shown
   * when the sheet opens. */
  // svelte-ignore state_referenced_locally
  let unfolded: Record<string, boolean> = $state(
    Object.fromEntries(ancestors(currentTag?.name ?? '').map((name) => [tagKey(name), true])),
  )
  function ancestors(name: string): string[] {
    const out: string[] = []
    for (let parent = tagParent(name); parent; parent = tagParent(parent)) out.push(parent)
    return out
  }

  /** Letters without their accents, so "e" finds "é". */
  const plain = (text: string) => text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase()
  const found = $derived.by(() => {
    const words = plain(query.trim())
    if (!words) return null
    return workspace.tags.filter((tag) => plain(tag.name).includes(words))
  })

  function go(scope: Scope) {
    void workspace.setScope(scope)
    close()
  }
  const here = (scope: Scope) =>
    current.kind === scope.kind &&
    ('id' in current ? current.id : null) === ('id' in scope ? scope.id : null)
</script>

{#snippet place(scope: Scope, icon: IconName, label: string)}
  <button
    class="row"
    class:here={here(scope)}
    aria-current={here(scope) ? 'true' : undefined}
    onclick={() => go(scope)}
  >
    <Icon name={icon} size={20} /><span class="name">{label}</span>
  </button>
{/snippet}

{#snippet mark(open: boolean)}
  {#if prefs.tagMark === 'hash'}
    <Icon name="hash" size={18} />
  {:else}
    <Icon name={open ? 'folderOpen' : 'folder'} size={18} />
  {/if}
{/snippet}

{#snippet tree(parent: string, depth: number)}
  {#each childrenOf(parent) as tag (tag.id)}
    {@const open = holds(tag) && !!unfolded[tagKey(tag.name)]}
    {@const scope: Scope = { kind: 'tag', id: tag.id }}
    <div class="line" style:padding-left="{depth * 24}px">
      {#if holds(tag)}
        <button
          class="fold"
          aria-label={open ? t('tags.collapse') : t('tags.expand')}
          aria-expanded={open}
          onclick={() => (unfolded[tagKey(tag.name)] = !open)}
        >
          <Icon name={open ? 'down' : 'right'} size={18} />
        </button>
      {:else}
        <span class="fold"></span>
      {/if}
      <button
        class="row"
        class:here={here(scope)}
        aria-current={here(scope) ? 'true' : undefined}
        onclick={() => go(scope)}
      >
        {@render mark(open)}
        <span class="name">{tagLeaf(tag.name)}</span>
        {#if tag.color}<span class="tag-mark" data-color={tag.color}></span>{/if}
        <span class="count">{tag.noteCount}</span>
      </button>
    </div>
    {#if open}{@render tree(tag.name, depth + 1)}{/if}
  {/each}
{/snippet}

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div
    class="sheet"
    bind:this={sheet}
    style:min-height={query.trim() && openHeight ? `${openHeight}px` : undefined}
    role="dialog"
    aria-modal="true"
    aria-label={t('list.scopeLabel')}
    use:modal={{ onclose: close }}
  >
    <div class="grab" aria-hidden="true"></div>
    <label class="find">
      <Icon name="search" size={18} />
      <input
        type="search"
        placeholder={t('tags.find')}
        aria-label={t('tags.find')}
        autocomplete="off"
        bind:value={query}
        onfocus={() => (openHeight = sheet?.offsetHeight ?? 0)}
      />
    </label>
    <div class="rows">
      {#if found}
        <p class="label">{t('tags.found', { count: found.length })}</p>
        {#each found as tag (tag.id)}
          {@const scope: Scope = { kind: 'tag', id: tag.id }}
          <button
            class="row"
            class:here={here(scope)}
            aria-current={here(scope) ? 'true' : undefined}
            onclick={() => go(scope)}
          >
            {@render mark(false)}
            <span class="name two">
              <span>{tagLeaf(tag.name)}</span>
              {#if tagParent(tag.name)}<span class="where">{tagLabel(tagParent(tag.name))}</span
                >{/if}
            </span>
            {#if tag.color}<span class="tag-mark" data-color={tag.color}></span>{/if}
            <span class="count">{tag.noteCount}</span>
          </button>
        {/each}
      {:else}
        {@render place(UNFILED, 'inbox', t('sidebar.untagged'))}
        {@render place({ kind: 'all' }, 'allNotes', t('sidebar.allNotes'))}
        {#if workspace.tags.length}
          <p class="label">{t('sidebar.tags')}</p>
          {@render tree('', 0)}
        {/if}
        <hr />
        {@render place({ kind: 'trash' }, 'trash', t('sidebar.trash'))}
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    /* over the floating feedback button, under menus and dialogs */
    z-index: 45;
    display: flex;
    align-items: flex-end;
    background: rgb(8 6 6 / 0.72);
    animation: dim var(--dur-base) var(--ease-out);
  }
  /* a sheet from the bottom, as the app's other phone prompts */
  .sheet {
    width: 100%;
    max-height: 86%;
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--color-border-strong);
    border-radius: var(--radius-window) var(--radius-window) 0 0;
    background: var(--color-surface);
    padding-bottom: env(safe-area-inset-bottom);
    animation: rise var(--dur-base) var(--ease-out);
  }
  .grab {
    width: 36px;
    height: 4px;
    margin: var(--space-2) auto var(--space-1);
    border-radius: 2px;
    background: var(--color-border-strong);
  }
  .find {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 44px;
    margin: var(--space-2) var(--space-4);
    padding: 0 var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-control);
    background: var(--color-bg);
    color: var(--color-text-muted);
  }
  .find:focus-within {
    border-color: var(--color-accent);
  }
  .find input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: 16px;
  }
  .rows {
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 0 var(--space-2) var(--space-4);
  }
  .label {
    margin: 0;
    padding: 14px var(--space-3) 6px;
    color: var(--color-text-muted);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .line {
    display: flex;
    align-items: center;
  }
  .row {
    flex: 1;
    min-width: 0;
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 48px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--color-text);
    font: inherit;
    font-size: 15px;
    text-align: left;
  }
  .row :global(svg) {
    flex: none;
    color: var(--color-text-muted);
  }
  .row.here {
    background: var(--color-selection);
  }
  .row.here :global(svg) {
    color: var(--color-accent-text);
  }
  .line .row {
    padding-left: var(--space-1);
  }
  .fold {
    flex: none;
    width: 36px;
    height: 48px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-text-muted);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.two {
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }
  .where {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .count {
    color: var(--color-text-muted);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  hr {
    margin: 6px var(--space-3);
    border: 0;
    border-top: 1px solid var(--color-border);
  }
  @keyframes dim {
    from {
      opacity: 0;
    }
  }
  @keyframes rise {
    from {
      transform: translateY(100%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .backdrop,
    .sheet {
      animation: none;
    }
  }
</style>
