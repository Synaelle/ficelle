<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Formatting buttons. Sticks to the top of the note while scrolling. On
     touch screens it sits at the bottom instead, which is just above the
     keyboard since the page shrinks when the keyboard opens, and shows Undo
     and the five most used: Redo and the rest are in a sheet behind More,
     with their names. -->
<script lang="ts">
  import type { Editor } from '@tiptap/core'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { prefs, shortcutFor } from '../lib/prefs.svelte'
  import { displayBinding, type ActionId } from '../lib/shortcuts'

  let {
    editor,
    tick,
    onlink,
    onimage,
  }: {
    editor: Editor
    tick: number
    onlink: () => void
    /** None for a file outside the notes, which keeps no pictures. */
    onimage?: () => void
  } = $props()

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
      icon: 'inlineCode',
      label: () => t('editor.inlineCode'),
      action: 'code',
      run: () => chain().toggleCode().run(),
      active: () => editor.isActive('code'),
    },
    {
      icon: 'code',
      label: () => t('editor.code'),
      // the code shortcut makes inline code, a code block has none
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
    { icon: 'image', quick: true, label: () => t('editor.image'), run: () => onimage?.() },
  ]

  /** The buttons not hidden in the settings. */
  const kept = $derived(
    tools.filter(
      (tool) => !prefs.hiddenTools.includes(tool.icon) && (tool.icon !== 'image' || onimage),
    ),
  )
  // a phone keyboard has no undo key: Undo on the bar, Redo first behind More
  const undo: Tool = {
    icon: 'undo',
    label: () => t('editor.undo'),
    run: () => chain().undo().run(),
    disabled: () => !editor.can().undo(),
  }
  const redo: Tool = {
    icon: 'redo',
    label: () => t('editor.redo'),
    run: () => chain().redo().run(),
    disabled: () => !editor.can().redo(),
  }
  const shown = $derived(TOUCH ? [undo, ...kept.filter((tool) => tool.quick)] : kept)

  function more() {
    overlay.openSheet(
      [redo, ...kept.filter((tool) => !tool.quick)].map((tool) => ({
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
    /* the header's 32 px buttons, side by side so the row stays one line
       as long as it did with smaller ones */
    gap: 0;
    margin-bottom: var(--space-3);
    padding: var(--space-1) 0;
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
  }
  /* stuck, the bar can sit a pixel or two under the pane's top edge
     (WebKit rounds it): the text scrolling past is covered up to the edge */
  .toolbar::before {
    content: '';
    position: absolute;
    inset: auto 0 100%;
    height: var(--space-2);
    background: inherit;
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
    /* on pure black the bar's top edge is all that sets it off the note */
    :global(:root[data-black='true']) .toolbar {
      border-top-color: var(--color-border-strong);
    }
    .toolbar::before {
      content: none;
    }
    /* a phone's width shared out, but no wider than a thumb needs: on its
       side the buttons stayed apart by a hand's width */
    .toolbar :global(.icon-btn) {
      flex: 1 1 44px;
      max-width: 64px;
      height: 44px;
    }
    .divider {
      flex: none;
      margin-left: auto;
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
