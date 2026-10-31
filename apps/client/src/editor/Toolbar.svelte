<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Formatting buttons. Sticks to the top of the note while scrolling. On
     touch screens it sits at the bottom instead, which is just above the
     keyboard since the page shrinks when the keyboard opens. -->
<script lang="ts">
  import type { Editor } from '@tiptap/core'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
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
  }

  const chain = () => editor.chain().focus()
  const tools: Tool[] = [
    {
      icon: 'bold',
      label: () => t('editor.bold'),
      action: 'bold',
      run: () => chain().toggleBold().run(),
      active: () => editor.isActive('bold'),
    },
    {
      icon: 'italic',
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
      label: () => t('editor.heading'),
      action: 'heading',
      run: () => chain().toggleHeading({ level: 2 }).run(),
      active: () => editor.isActive('heading'),
    },
    {
      icon: 'list',
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
    },
    { icon: 'image', label: () => t('editor.image'), run: () => onimage() },
  ]

  // On touch screens the bar scrolls sideways. A faded edge shows that more
  // buttons wait on that side, a cut button alone looks like a layout bug.
  let bar: HTMLDivElement | undefined = $state()
  let moreLeft = $state(false)
  let moreRight = $state(false)
  function edges() {
    if (!bar) return
    moreLeft = bar.scrollLeft > 1
    moreRight = bar.scrollLeft + bar.clientWidth < bar.scrollWidth - 1
  }
  $effect(() => {
    if (!bar) return
    edges()
    const observer = new ResizeObserver(edges)
    observer.observe(bar)
    return () => observer.disconnect()
  })

  function title(tool: Tool) {
    const label = tool.label()
    return tool.action ? `${label} (${displayBinding(shortcutFor(tool.action))})` : label
  }
</script>

<div
  class="toolbar"
  class:more-left={moreLeft}
  class:more-right={moreRight}
  role="toolbar"
  aria-label={t('editor.toolbar')}
  bind:this={bar}
  onscroll={edges}
>
  {#each tools as tool (tool.icon)}
    <button
      class="icon-btn"
      aria-label={tool.label()}
      title={title(tool)}
      aria-pressed={tool.active ? (void tick, tool.active()) : undefined}
      onmousedown={(e) => e.preventDefault()}
      onpointerdown={(e) => e.pointerType === 'touch' && e.preventDefault()}
      onclick={tool.run}
    >
      <Icon name={tool.icon} size={17} />
    </button>
  {/each}
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
      overflow-x: auto;
      margin: 0;
      padding: var(--space-1) var(--space-2) calc(var(--space-1) + env(safe-area-inset-bottom));
      border-top: 1px solid var(--color-border);
      border-bottom: 0;
      scrollbar-width: none;
      --fade-left: 0px;
      --fade-right: 0px;
      mask-image: linear-gradient(
        to right,
        transparent,
        #000 var(--fade-left),
        #000 calc(100% - var(--fade-right)),
        transparent
      );
    }
    .toolbar.more-left {
      --fade-left: 40px;
    }
    .toolbar.more-right {
      --fade-right: 40px;
    }
    .toolbar :global(.icon-btn) {
      flex: none;
      width: 44px;
      height: 44px;
    }
  }
</style>
