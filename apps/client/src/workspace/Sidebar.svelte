<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Standard layout only: all notes, folders, tags, trash and settings. -->
<script lang="ts">
  import { sync } from '../lib/sync.svelte'
  import SyncIcon from './SyncIcon.svelte'
  import type { Scope } from '../lib/backend'
  import { t } from '../lib/i18n.svelte'
  import { noteDropTarget } from '../lib/noteDrag'
  import { prefs } from '../lib/prefs.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { overlay } from '../lib/overlay.svelte'
  import { workspace } from '../lib/workspace.svelte'
  import FolderTree from './FolderTree.svelte'
  import { sharing } from '../lib/sharing.svelte'

  let {
    onsettings,
    onlock,
    lockable,
  }: { onsettings: (section?: 'sync') => void; onlock: () => void; lockable: boolean } = $props()

  const same = (a: Scope, b: Scope) =>
    a.kind === b.kind && ('id' in a ? a.id : null) === ('id' in b ? b.id : null)

  // the important tag has its own place above, not twice

  let newFolderName = $state('')
  let addingFolder = $state(false)

  async function addFolder(event: SubmitEvent) {
    event.preventDefault()
    if (newFolderName.trim()) await workspace.createFolder(null, newFolderName)
    newFolderName = ''
    addingFolder = false
  }
</script>

{#snippet item(scope: Scope, icon: IconName, label: string, count?: number)}
  <button
    class="item"
    class:active={!workspace.files && same(workspace.scope, scope)}
    aria-current={!workspace.files && same(workspace.scope, scope) ? 'page' : undefined}
    onclick={() =>
      // a second click on the active filter goes back to every note
      workspace.setScope(same(workspace.scope, scope) ? { kind: 'all' } : scope)}
  >
    <Icon name={icon} size={16} />
    <span class="label">{label}</span>
    {#if count !== undefined}<span class="count">{count}</span>{/if}
  </button>
{/snippet}

<nav class="sidebar" aria-label={t('sidebar.label')} style:width="{prefs.sidebarWidth}px">
  <div class="group">
    {@render item({ kind: 'all' }, 'inbox', t('sidebar.allNotes'))}
  </div>

  <div class="group">
    <!-- dropping a note on the heading takes it out of its folder -->
    <div class="heading" use:noteDropTarget={(id) => void workspace.move(id, null)}>
      <span>{t('sidebar.folders')}</span>
      <button
        class="icon-btn small"
        aria-label={t('sidebar.newFolder')}
        onclick={() => (addingFolder = true)}
      >
        <Icon name="plus" size={14} />
      </button>
    </div>
    <FolderTree parent={null} depth={0} />
    {#if addingFolder}
      <form onsubmit={addFolder}>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="field"
          autofocus
          placeholder={t('sidebar.folderName')}
          bind:value={newFolderName}
          onblur={() => (addingFolder = false)}
        />
      </form>
    {/if}
  </div>

  {#if sharing.list.length}
    <div class="group">
      <div class="heading"><span>{t('sidebar.shared')}</span></div>
      {#each sharing.list as space (space.id)}
        <div
          class="shared"
          title={space.role === 'owner'
            ? t('share.byYou')
            : t('share.by', { owner: space.ownerName })}
        >
          <FolderTree parent={null} depth={0} space={space.id} />
          {#each space.notes as note (note.id)}
            <button
              class="item"
              class:active={workspace.current?.id === note.id}
              onclick={() => void workspace.open(note.id)}
              oncontextmenu={(e) =>
                overlay.openMenu(e, [
                  {
                    label: t('share.sharing'),
                    icon: 'share',
                    run: () =>
                      sharing.open({ space: space.id, name: note.title || t('note.untitled') }),
                  },
                ])}
            >
              <Icon name="share" size={16} />
              <span class="label">{note.title || t('note.untitled')}</span>
            </button>
          {/each}
          {#if space.role !== 'owner'}
            <span class="owner">{t('share.by', { owner: space.ownerName })}</span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  {#if workspace.tags.length}
    <div class="group">
      <div class="heading"><span>{t('sidebar.tags')}</span></div>
      {#each workspace.tags as tag (tag.id)}
        {@render item({ kind: 'tag', id: tag.id }, 'tag', tag.name, tag.noteCount)}
      {/each}
    </div>
  {/if}

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
  .owner {
    padding: 0 var(--space-2) var(--space-1) 26px;
    font-size: 12px;
    color: var(--color-text-muted);
  }
</style>
