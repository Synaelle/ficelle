<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Formatting buttons. Sticks to the top of the note while scrolling. On
     touch screens it sits at the bottom instead, which is just above the
     keyboard since the page shrinks when the keyboard opens, and shows the
     five most used: the rest are in a sheet behind More, with their names. -->
<script lang="ts">
  import type { Editor } from '@tiptap/core'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { shortcutFor } from '../lib/prefs.svelte'
  import { displayBinding, type ActionId } from '../lib/shortcuts'

  let {
    editor,
    tick,
    onlink,
    onimage,
  }: { editor: Editor; tick: number; onlink: () => void; onimage: () => void } = $props()

  interface Tool {
    icon: IconName
    label: () => string
    action?: ActionId
    run: () => void
    active?: () => boolean
    disabled?: () => boolean
    /** On the touch bar itself, the others behind More. */
    quick?: true
  }

  const TOUCH = window.matchMedia('(pointer: coarse)').matches

  const chain = () => editor.chain().focus()
  const tools: Tool[] = [
    {
      icon: 'bold',
      quick: true,
      label: () => t('editor.bold'),
      action: 'bold',
      run: () => chain().toggleBold().run(),
      active: () => editor.isActive('bold'),
    },
    {
      icon: 'italic',
      quick: true,
      label: () => t('editor.italic'),
      action: 'italic',
      run: () => chain().toggleItalic().run(),
      active: () => editor.isActive('italic'),
    },
    {
      icon: 'strike',
      label: () => t('editor.strike'),
      action: 'strike',
      run: () => chain().toggleStrike().run(),
      active: () => editor.isActive('strike'),
    },
    {
      icon: 'heading',
      quick: true,
      label: () => t('editor.heading'),
      run: () => chain().toggleHeading({ level: 2 }).run(),
      active: () => editor.isActive('heading'),
    },
    {
      icon: 'list',
      quick: true,
      label: () => t('editor.bulletList'),
      run: () => chain().toggleBulletList().run(),
      active: () => editor.isActive('bulletList'),
    },
    {
      icon: 'listOrdered',
      label: () => t('editor.orderedList'),
      run: () => chain().toggleOrderedList().run(),
      active: () => editor.isActive('orderedList'),
    },
    {
      icon: 'tasks',
      quick: true,
      label: () => t('editor.checklist'),
      action: 'checklist',
      run: () => chain().toggleTaskList().run(),
      active: () => editor.isActive('taskList'),
    },
    {
      icon: 'quote',
      label: () => t('editor.quote'),
      run: () => chain().toggleBlockquote().run(),
      active: () => editor.isActive('blockquote'),
    },
    {
      icon: 'code',
      label: () => t('editor.code'),
      action: 'code',
      run: () => chain().toggleCodeBlock().run(),
      active: () => editor.isActive('codeBlock'),
    },
    {
      icon: 'link',
      label: () => t('editor.link'),
      action: 'link',
      run: () => onlink(),
      active: () => editor.isActive('link'),
    },
    {
      icon: 'table',
      label: () => t('editor.table'),
      run: () => chain().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(),
      // a table holds no table, a Markdown file could not keep it
      disabled: () => editor.isActive('table'),
    },
    { icon: 'image', label: () => t('editor.image'), run: () => onimage() },
  ]

  const shown = TOUCH ? tools.filter((tool) => tool.quick) : tools

  function more() {
    overlay.openSheet(
      tools
        .filter((tool) => !tool.quick)
        .map((tool) => ({
          label: tool.label(),
          icon: tool.icon,
          disabled: tool.disabled?.(),
          run: tool.run,
        })),
    )
  }

  function title(tool: Tool) {
    const label = tool.label()
    return tool.action ? `${label} (${displayBinding(shortcutFor(tool.action))})` : label
  }
</script>

<div class="toolbar" role="toolbar" aria-label={t('editor.toolbar')}>
  {#each shown as tool (tool.icon)}
    <button
      class="icon-btn"
      aria-label={tool.label()}
      title={title(tool)}
      aria-pressed={tool.active ? (void tick, tool.active()) : undefined}
      disabled={tool.disabled ? (void tick, tool.disabled()) : undefined}
      onmousedown={(e) => e.preventDefault()}
      onpointerdown={(e) => e.pointerType === 'touch' && e.preventDefault()}
      onclick={tool.run}
    >
      <Icon name={tool.icon} size={17} />
    </button>
  {/each}
  {#if TOUCH}
    <span class="divider" aria-hidden="true"></span>
    <button
      class="more"
      aria-label={t('editor.moreLabel')}
      aria-haspopup="menu"
      onpointerdown={(e) => e.pointerType === 'touch' && e.preventDefault()}
      onclick={more}>{t('editor.more')}</button
    >
  {/if}
</div>

<style>
  .toolbar {
    position: sticky;
    top: 0;
    z-index: 4;
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    margin-bottom: var(--space-3);
    padding: var(--space-1) 0;
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
  }
  .toolbar :global(.icon-btn) {
    width: 30px;
    height: 30px;
  }
  @media (pointer: coarse) {
    .toolbar {
      position: fixed;
      top: auto;
      bottom: 0;
      left: 0;
      right: 0;
      z-index: 30;
      flex-wrap: nowrap;
      align-items: center;
      margin: 0;
      padding: var(--space-1) var(--space-2) calc(var(--space-1) + env(safe-area-inset-bottom));
      border-top: 1px solid var(--color-border);
      border-bottom: 0;
    }
    .toolbar :global(.icon-btn) {
      flex: 1 1 44px;
      height: 44px;
    }
    .divider {
      flex: none;
      width: 1px;
      height: 24px;
      background: var(--color-border);
    }
    .more {
      flex: none;
      min-height: 44px;
      padding: 0 var(--space-3);
      border: 0;
      border-radius: var(--radius-control);
      background: none;
      color: var(--color-text-muted);
      font: inherit;
      font-size: 14px;
    }
  }
</style>
