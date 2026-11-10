<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A table's own tools, shown while the caret is in it, so the formatting
     toolbar stays the same everywhere. On a computer a small bar sits above
     the table's right corner. On a phone, where the toolbar is down by the
     keyboard and a bar would cover the next lines, a button in the corner of
     the current cell opens them all as a sheet. -->
<script lang="ts">
  import type { Editor } from '@tiptap/core'
  import { Plugin, PluginKey } from '@tiptap/pm/state'
  import { Decoration, DecorationSet } from '@tiptap/pm/view'
  import { onMount } from 'svelte'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { overlay, SEPARATOR, type MenuItem } from '../lib/overlay.svelte'
  import { MOBILE } from '../lib/platform'

  let { editor, tick }: { editor: Editor; tick: number } = $props()

  const chain = () => editor.chain().focus()
  const adds: { icon: IconName; label: () => string; run: () => void }[] = [
    {
      icon: 'rowAbove',
      label: () => t('editor.rowAbove'),
      run: () => chain().addRowBefore().run(),
    },
    { icon: 'rowBelow', label: () => t('editor.rowBelow'), run: () => chain().addRowAfter().run() },
    {
      icon: 'columnLeft',
      label: () => t('editor.columnLeft'),
      run: () => chain().addColumnBefore().run(),
    },
    {
      icon: 'columnRight',
      label: () => t('editor.columnRight'),
      run: () => chain().addColumnAfter().run(),
    },
  ]
  const removes = (): MenuItem[] => [
    { label: t('editor.deleteRow'), icon: 'trash', run: () => chain().deleteRow().run() },
    { label: t('editor.deleteColumn'), icon: 'trash', run: () => chain().deleteColumn().run() },
    SEPARATOR,
    {
      label: t('editor.deleteTable'),
      icon: 'trash',
      danger: true,
      run: () => chain().deleteTable().run(),
    },
  ]

  /** The table and the cell the caret is in, as the page draws them. */
  const where = $derived.by(() => {
    void tick
    const from = editor.state.selection.$from
    let table: HTMLElement | null = null
    let cell: HTMLElement | null = null
    for (let depth = from.depth; depth > 0; depth--) {
      const name = from.node(depth).type.name
      const dom = editor.view.nodeDOM(from.before(depth))
      if (!(dom instanceof HTMLElement)) continue
      if (!cell && (name === 'tableCell' || name === 'tableHeader')) cell = dom
      if (name === 'table') {
        table = dom
        break
      }
    }
    return table ? { table, cell } : null
  })

  /** The note's surface, where the bars are placed: the editor is mounted in it. */
  const host = $derived(editor.view.dom.parentElement)

  let bar: HTMLDivElement | undefined = $state()
  const barStyle = $derived.by(() => {
    if (!where || !host) return ''
    const box = where.table.getBoundingClientRect()
    const frame = host.getBoundingClientRect()
    const width = bar?.offsetWidth ?? 0
    // above the table's right corner, reaching into the gap under the
    // toolbar (--space-3) if needed, or under the table when it starts the
    // note: never over the header row
    const top = box.top - frame.top - 38
    const left = Math.max(0, box.right - frame.left - width)
    return `top:${top < -12 ? box.bottom - frame.top + 4 : top}px;left:${left}px`
  })
  const cellStyle = $derived.by(() => {
    if (!where?.cell || !host) return ''
    const box = where.cell.getBoundingClientRect()
    const frame = host.getBoundingClientRect()
    return `top:${box.top - frame.top + 4}px;left:${box.right - frame.left - 32}px`
  })

  // presses keep the caret in the cell, and the keyboard open on a phone
  const keep = (e: PointerEvent | MouseEvent) => e.preventDefault()

  // on a phone the button sits in the cell: the cell being edited makes room
  // for it, so a narrow column's text stays readable
  const roomKey = new PluginKey('tableCellRoom')
  onMount(() => {
    if (!MOBILE) return
    editor.registerPlugin(
      new Plugin({
        key: roomKey,
        props: {
          decorations: (state) => {
            const from = state.selection.$from
            for (let depth = from.depth; depth > 0; depth--) {
              const node = from.node(depth)
              if (node.type.name !== 'tableCell' && node.type.name !== 'tableHeader') continue
              const at = from.before(depth)
              return DecorationSet.create(state.doc, [
                Decoration.node(at, at + node.nodeSize, { class: 'cell-with-tools' }),
              ])
            }
            return null
          },
        },
      }),
    )
    return () => {
      if (!editor.isDestroyed) editor.unregisterPlugin(roomKey)
    }
  })
</script>

{#if where && MOBILE}
  {#if where.cell}
    <button
      class="cell-btn"
      style={cellStyle}
      aria-label={t('editor.tableMenu')}
      aria-haspopup="menu"
      onmousedown={keep}
      onpointerdown={keep}
      onclick={(e) =>
        overlay.openMenu(e, [
          ...adds.map((tool) => ({ label: tool.label(), icon: tool.icon, run: tool.run })),
          SEPARATOR,
          ...removes(),
        ])}
    >
      <Icon name="down" size={16} />
    </button>
  {/if}
{:else if where}
  <div class="bar" style={barStyle} bind:this={bar} role="toolbar" aria-label={t('editor.table')}>
    {#each adds as tool (tool.icon)}
      <button
        class="icon-btn"
        aria-label={tool.label()}
        title={tool.label()}
        onmousedown={keep}
        onclick={tool.run}
      >
        <Icon name={tool.icon} size={16} />
      </button>
    {/each}
    <span class="sep" aria-hidden="true"></span>
    <button
      class="icon-btn"
      aria-label={t('editor.tableRemove')}
      title={t('editor.tableRemove')}
      aria-haspopup="menu"
      onmousedown={keep}
      onclick={(e) => overlay.openMenuBelow(e.currentTarget, removes())}
    >
      <Icon name="trash" size={16} />
    </button>
  </div>
{/if}

<style>
  .bar {
    position: absolute;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
  }
  .bar :global(.icon-btn) {
    width: 30px;
    height: 30px;
  }
  .sep {
    width: 1px;
    height: 18px;
    margin: 0 var(--space-1);
    background: var(--color-border-strong);
  }
  :global(.ProseMirror .cell-with-tools) {
    padding-right: 36px;
  }
  .cell-btn {
    position: absolute;
    z-index: 5;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    color: var(--color-text);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-control);
  }
</style>
