<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Standard layout only: notes with no tag, all notes, the tag tree,
     trash and settings. -->
<script lang="ts">
  import { localSync } from '../lib/localSync.svelte'
  import { sync } from '../lib/sync.svelte'
  import SyncIcon from './SyncIcon.svelte'
  import type { Scope } from '../lib/backend'
  import { t } from '../lib/i18n.svelte'
  import { noteDropTarget } from '../lib/noteDrag'
  import { prefs } from '../lib/prefs.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { UNFILED, workspace } from '../lib/workspace.svelte'
  import TagTree from './TagTree.svelte'
  import TagsDialog from './TagsDialog.svelte'

  let {
    onsettings,
    onlock,
    lockable,
  }: { onsettings: (section?: 'sync') => void; onlock: () => void; lockable: boolean } = $props()

  const same = (a: Scope, b: Scope) =>
    a.kind === b.kind && ('id' in a ? a.id : null) === ('id' in b ? b.id : null)

  let newTagName = $state('')
  let addingTag = $state(false)
  let managing = $state(false)
  /** Tags folded in the tree, by lowercased path. */
  let collapsed: Record<string, boolean> = $state({})

  async function addTag(event: SubmitEvent) {
    event.preventDefault()
    const name = newTagName
    newTagName = ''
    addingTag = false
    if (!name.trim()) return
    try {
      await workspace.createTag(name)
    } catch (err) {
      workspace.report(err)
    }
  }
</script>

{#snippet item(scope: Scope, icon: IconName, label: string, count?: number)}
  <button
    class="item"
    class:active={!workspace.files && same(workspace.scope, scope)}
    aria-current={!workspace.files && same(workspace.scope, scope) ? 'page' : undefined}
    onclick={() =>
      // a second click on the active filter goes back to the notes with no tag
      workspace.setScope(same(workspace.scope, scope) ? UNFILED : scope)}
  >
    <Icon name={icon} size={16} />
    <span class="label">{label}</span>
    {#if count !== undefined}<span class="count">{count}</span>{/if}
  </button>
{/snippet}

<nav class="sidebar" aria-label={t('sidebar.label')} style:width="{prefs.sidebarWidth}px">
  <div class="group">
    {@render item(UNFILED, 'inbox', t('sidebar.untagged'))}
    {@render item({ kind: 'all' }, 'allNotes', t('sidebar.allNotes'))}
  </div>

  <div class="group">
    <div class="heading">
      <span>{t('sidebar.tags')}</span>
      <button class="icon-btn small" aria-label={t('tags.new')} onclick={() => (addingTag = true)}>
        <Icon name="plus" size={14} />
      </button>
    </div>
    <TagTree parent="" depth={0} bind:collapsed />
    {#if addingTag}
      <form onsubmit={addTag}>
        <!-- autofocus would leave the focus on the + button, which keeps it -->
        <input
          class="field"
          {@attach (field) => field.focus()}
          placeholder={t('tags.name')}
          bind:value={newTagName}
          onblur={() => (addingTag = false)}
        />
      </form>
    {/if}
    {#if workspace.tags.length}
      <button class="manage" onclick={() => (managing = true)}>{t('tags.manage')}</button>
    {/if}
  </div>

  <div class="spacer"></div>
  <div class="group">
    <button
      class="item"
      class:active={workspace.files}
      aria-current={workspace.files ? 'page' : undefined}
      onclick={() => (workspace.files = !workspace.files)}
      ><Icon name="attachment" size={16} /><span class="label">{t('sidebar.files')}</span></button
    >
    <div use:noteDropTarget={(id) => void workspace.trash(id)}>
      {@render item({ kind: 'trash' }, 'trash', t('sidebar.trash'))}
    </div>
    {#if sync.status.state !== 'off'}
      <button class="item" onclick={() => onsettings('sync')} title={sync.status.error ?? ''}
        ><SyncIcon state={sync.status.state} />{t(`sync.state.${sync.status.state}`)}</button
      >
    {:else if localSync.summary}
      {@const line = t(`local.status.${localSync.summary.key}`, { name: localSync.summary.name })}
      <button class="item" onclick={() => onsettings('sync')} title={line}
        ><SyncIcon state={localSync.summary.state} /><span class="label">{line}</span></button
      >
    {/if}
    <button class="item" onclick={() => onsettings()}
      ><Icon name="settings" size={16} />{t('sidebar.settings')}</button
    >
    {#if lockable}
      <button class="item" onclick={onlock}
        ><Icon name="lock" size={16} />{t('sidebar.lock')}</button
      >
    {/if}
  </div>
</nav>

{#if managing}
  <TagsDialog close={() => (managing = false)} />
{/if}

<style>
  .sidebar {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-2);
    background: var(--color-bg);
    border-right: 1px solid var(--color-border);
    overflow-y: auto;
  }
  .group {
    display: grid;
    gap: 1px;
  }
  .heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 var(--space-2);
    font-size: 12px;
    color: var(--color-text-muted);
  }
  .small {
    width: 24px;
    height: 24px;
  }
  .spacer {
    flex: 1;
  }
  .item,
  .sidebar :global(.tree-item) {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: 30px;
    padding: 0 var(--space-2);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--color-text);
    text-align: left;
  }
  @media (hover: hover) {
    .item:hover,
    .sidebar :global(.tree-item:hover) {
      background: var(--color-surface-2);
    }
  }
  .item.active,
  .sidebar :global(.tree-item.active) {
    background: var(--color-selection);
    color: var(--color-text);
  }
  .item.active :global(.icon),
  .sidebar :global(.tree-item.active .icon) {
    color: var(--color-accent-text);
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 12px;
    color: var(--color-text-muted);
  }
  form {
    padding: var(--space-1) var(--space-2);
  }
  .manage {
    justify-self: start;
    padding: var(--space-1) var(--space-2);
    border: 0;
    background: none;
    font-size: 12px;
    color: var(--color-text-muted);
  }
  @media (hover: hover) {
    .manage:hover {
      color: var(--color-text);
    }
  }
</style>
