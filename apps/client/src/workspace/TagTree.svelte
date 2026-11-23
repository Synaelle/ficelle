<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The tags under one tag, recursively, as the sidebar shows them. A note
     dropped on a tag gets it, a double click renames. -->
<script lang="ts">
  import type { Tag } from '../lib/backend'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { noteDropTarget, startTagDrag, tagDropTarget } from '../lib/noteDrag'
  import { overlay } from '../lib/overlay.svelte'
  import { tagMenuItems } from '../lib/tagMenu'
  import { tagLeaf, tagParent } from '../lib/tagMark.svelte'
  import { moveTag, tagKey, UNFILED, workspace } from '../lib/workspace.svelte'
  import TagTree from './TagTree.svelte'

  let {
    parent,
    depth,
    collapsed = $bindable({}),
  }: { parent: string; depth: number; collapsed?: Record<string, boolean> } = $props()

  const children = $derived(
    workspace.tags.filter((tag) => tagKey(tagParent(tag.name)) === tagKey(parent)),
  )
  const holds = (tag: Tag) =>
    workspace.tags.some((o) => tagKey(tagParent(o.name)) === tagKey(tag.name))
  let renaming: string | null = $state(null)
  let draft = $state('')

  async function commitRename(tag: Tag) {
    const name = draft.trim()
    renaming = null
    if (!name || name === tagLeaf(tag.name)) return
    try {
      await workspace.renameTag(tag.id, parent ? `${parent}/${name}` : name)
    } catch (err) {
      workspace.report(err)
    }
  }

  function oncontextmenu(event: MouseEvent, tag: Tag) {
    const items = tagMenuItems(tag, {
      rename: () => {
        renaming = tag.id
        draft = tagLeaf(tag.name)
      },
      attempt: async (change) => {
        try {
          await change()
        } catch (err) {
          workspace.report(err)
        }
      },
      unfold: () => (collapsed[tagKey(tag.name)] = false),
    })
    overlay.openMenu(event, items)
  }
</script>

{#each children as tag (tag.id)}
  {@const active = workspace.scope.kind === 'tag' && workspace.scope.id === tag.id}
  {@const open = holds(tag) && !collapsed[tagKey(tag.name)]}
  <div class="row" style:padding-left="{depth * 12}px">
    {#if holds(tag)}
      <button
        class="toggle"
        aria-label={open ? t('tags.collapse') : t('tags.expand')}
        aria-expanded={open}
        onclick={() => (collapsed[tagKey(tag.name)] = open)}
      >
        <Icon name={open ? 'down' : 'right'} size={12} />
      </button>
    {:else}
      <span class="toggle"></span>
    {/if}
    {#if renaming === tag.id}
      <input
        class="field"
        {@attach (field) => field.focus()}
        aria-label={t('tags.rename')}
        bind:value={draft}
        onblur={() => commitRename(tag)}
        onkeydown={(e) => {
          if (e.key === 'Enter') void commitRename(tag)
          if (e.key === 'Escape') renaming = null
        }}
      />
    {:else}
      <button
        class="tree-item"
        class:active
        aria-current={active ? 'page' : undefined}
        onclick={() => workspace.setScope(active ? UNFILED : { kind: 'tag', id: tag.id })}
        use:noteDropTarget={(id) => void workspace.addTag(id, tag.name)}
        use:tagDropTarget={(name) => void moveTag(name, tag.name)}
        draggable="true"
        ondragstart={(e) => startTagDrag(e, tag.name)}
        ondblclick={() => {
          renaming = tag.id
          draft = tagLeaf(tag.name)
        }}
        oncontextmenu={(e) => oncontextmenu(e, tag)}
      >
        <Icon name={open ? 'folderOpen' : 'folder'} size={16} />
        <span class="name" title={tag.name}>{tagLeaf(tag.name)}</span>
        {#if tag.color}<span class="tag-mark" data-color={tag.color}></span>{/if}
        <span class="count">{tag.noteCount}</span>
      </button>
    {/if}
  </div>
  {#if open}
    <TagTree parent={tag.name} depth={depth + 1} bind:collapsed />
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
  .count {
    margin-left: auto;
    font-size: 12px;
    color: var(--color-text-muted);
  }
</style>
