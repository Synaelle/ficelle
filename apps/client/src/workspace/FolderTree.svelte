<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Folders under one parent, recursively. Double click renames. -->
<script lang="ts">
  import type { Id } from '../lib/backend'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { noteDropTarget } from '../lib/noteDrag'
  import { overlay, SEPARATOR, type MenuItem } from '../lib/overlay.svelte'
  import { sharing } from '../lib/sharing.svelte'
  import { workspace } from '../lib/workspace.svelte'
  import FolderTree from './FolderTree.svelte'

  let { parent, depth, space = '' }: { parent: Id | null; depth: number; space?: string } = $props()

  // the top of a tree is one space's folders: personal ones under Folders,
  // each shared space under Shared
  const children = $derived(
    workspace.folders
      .filter((f) => f.parentId === parent && (parent !== null || f.spaceId === space))
      .sort((a, b) => a.position - b.position),
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

  function oncontextmenu(event: MouseEvent, id: Id, spaceId: string) {
    const items: MenuItem[] = []
    if (sharing.available && spaceId === '')
      items.push({
        label: t('share.shareItem'),
        icon: 'share',
        run: () => sharing.open({ item: { kind: 'folder', id }, name: workspace.folderName(id) }),
      })
    const root = sharing.rootOf(id)
    if (root)
      items.push({
        label: t('share.sharing'),
        icon: 'share',
        run: () => sharing.open({ space: root.id, name: workspace.folderName(id) }),
      })
    if (!sharing.readOnly(spaceId)) {
      if (items.length) items.push(SEPARATOR)
      items.push(
        { label: t('folders.newSubfolder'), icon: 'plus', run: () => void newSubfolder(id) },
        { label: t('folders.rename'), run: () => startRename(id) },
        SEPARATOR,
        { label: t('note.trash'), icon: 'trash', danger: true, run: () => void trashFolder(id) },
      )
    }
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
        onclick={() =>
          workspace.setScope(active ? { kind: 'all' } : { kind: 'folder', id: folder.id })}
        use:noteDropTarget={(id) => void workspace.move(id, folder.id)}
        ondblclick={() => !sharing.readOnly(folder.spaceId) && startRename(folder.id)}
        oncontextmenu={(e) => oncontextmenu(e, folder.id, folder.spaceId)}
      >
        <Icon name="folder" size={16} />
        <span class="name">{folder.name}</span>
      </button>
    {/if}
  </div>
  {#if !collapsed[folder.id]}
    <FolderTree parent={folder.id} depth={depth + 1} {space} />
  {/if}
{/each}

<style>
  .row {
    display: flex;
    align-items: center;
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
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
