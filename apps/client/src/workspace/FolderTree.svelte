<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Folders under one parent, recursively. Double click renames. -->
<script lang="ts">
  import type { Id } from '../lib/backend'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { noteDropTarget } from '../lib/noteDrag'
  import { overlay, SEPARATOR, type MenuItem } from '../lib/overlay.svelte'
  import { UNFILED, workspace } from '../lib/workspace.svelte'
  import FolderTree from './FolderTree.svelte'

  let { parent, depth }: { parent: Id | null; depth: number } = $props()

  const children = $derived(
    workspace.folders.filter((f) => f.parentId === parent).sort((a, b) => a.position - b.position),
  )
  let collapsed: Record<Id, boolean> = $state({})
  let renaming: Id | null = $state(null)
  let draft = $state('')

  const hasChildren = (id: Id) => workspace.folders.some((f) => f.parentId === id)

  async function commitRename(id: Id) {
    if (draft.trim()) await workspace.renameFolder(id, draft)
    renaming = null
  }

  function startRename(id: Id) {
    renaming = id
    draft = workspace.folderName(id)
  }

  async function newSubfolder(id: Id) {
    const name = await overlay.askText(t('sidebar.folderName'))
    if (name?.trim()) {
      collapsed[id] = false
      await workspace.createFolder(id, name)
    }
  }

  async function trashFolder(id: Id) {
    const yes = await overlay.ask(t('folders.confirmTrash', { name: workspace.folderName(id) }), {
      confirmLabel: t('note.trash'),
      danger: true,
    })
    if (yes) await workspace.trashFolder(id)
  }

  function oncontextmenu(event: MouseEvent, id: Id) {
    const items: MenuItem[] = [
      { label: t('folders.newSubfolder'), icon: 'plus', run: () => void newSubfolder(id) },
      { label: t('folders.rename'), run: () => startRename(id) },
      SEPARATOR,
      { label: t('note.trash'), icon: 'trash', danger: true, run: () => void trashFolder(id) },
    ]
    overlay.openMenu(event, items)
  }
</script>

{#each children as folder (folder.id)}
  {@const active = workspace.scope.kind === 'folder' && workspace.scope.id === folder.id}
  <div class="row" style:padding-left="{depth * 12}px">
    {#if hasChildren(folder.id)}
      <button
        class="toggle"
        aria-label={collapsed[folder.id] ? t('folders.expand') : t('folders.collapse')}
        aria-expanded={!collapsed[folder.id]}
        onclick={() => (collapsed[folder.id] = !collapsed[folder.id])}
      >
        <Icon name={collapsed[folder.id] ? 'right' : 'down'} size={12} />
      </button>
    {:else}
      <span class="toggle"></span>
    {/if}
    {#if renaming === folder.id}
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="field"
        autofocus
        bind:value={draft}
        onblur={() => commitRename(folder.id)}
        onkeydown={(e) => e.key === 'Enter' && commitRename(folder.id)}
      />
    {:else}
      <button
        class="tree-item"
        class:active
        aria-current={active ? 'page' : undefined}
        onclick={() => workspace.setScope(active ? UNFILED : { kind: 'folder', id: folder.id })}
        use:noteDropTarget={(id) => void workspace.move(id, folder.id)}
        ondblclick={() => startRename(folder.id)}
        oncontextmenu={(e) => oncontextmenu(e, folder.id)}
      >
        <Icon name="folder" size={16} />
        <span class="name" title={folder.name}>{folder.name}</span>
      </button>
    {/if}
  </div>
  {#if !collapsed[folder.id]}
    <FolderTree parent={folder.id} depth={depth + 1} />
  {/if}
{/each}

<style>
  .row {
    display: flex;
    align-items: center;
    min-width: 0;
  }
  .toggle {
    width: 16px;
    height: 30px;
    flex: none;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-text-muted);
  }
  /* a long name is cut, never wider than the sidebar */
  .row .tree-item {
    flex: 1;
    min-width: 0;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
