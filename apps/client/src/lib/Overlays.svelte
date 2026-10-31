<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Draws the dialogs and context menus of lib/overlay. Also replaces the
     webview menu: text fields get cut, copy and paste, the rest of the app
     shows a menu only where it has one. -->
<script lang="ts">
  import { tick } from 'svelte'
  import { PIN_LENGTH } from './backend'
  import { t } from './i18n.svelte'
  import Icon from './icons/Icon.svelte'
  import { modal } from './modal'
  import { overlay, textItems, type MenuItem } from './overlay.svelte'

  let menuElement: HTMLDivElement | undefined = $state()
  let position = $state({ left: 0, top: 0 })
  let textInput: HTMLInputElement | undefined = $state()
  let draft = $state('')

  const editable = (target: EventTarget | null): HTMLElement | null =>
    target instanceof HTMLElement
      ? target.closest<HTMLElement>(
          'input:not([type=checkbox]):not([type=radio]), textarea, [contenteditable=true]',
        )
      : null

  function oncontextmenu(event: MouseEvent) {
    if (event.defaultPrevented) return
    const field = editable(event.target)
    // a long touch in text belongs to the phone: its own selection handles
    // and copy and paste bar
    if (field && overlay.byTouch) return
    if (field) overlay.openMenu(event, textItems(field))
    else event.preventDefault()
  }

  // keep the menu inside the window
  $effect(() => {
    const menu = overlay.menu
    if (!menu) return
    if (menu.sheet) {
      void tick().then(() =>
        menuElement?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus(),
      )
      return
    }
    position = { left: menu.x, top: menu.y }
    void tick().then(() => {
      const box = menuElement?.getBoundingClientRect()
      if (!box) return
      const x = menu.alignRight ? menu.x - box.width : menu.x
      position = {
        left: Math.max(4, Math.min(x, window.innerWidth - box.width - 4)),
        top: Math.max(4, Math.min(menu.y, window.innerHeight - box.height - 4)),
      }
      menuElement?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()
    })
  })

  $effect(() => {
    const dialog = overlay.dialog
    if (dialog?.kind === 'text') {
      draft = dialog.value
      void tick().then(() => textInput?.select())
    }
  })

  function choose(item: MenuItem) {
    if (item.items) return overlay.enterSubmenu(item)
    overlay.closeMenu()
    item.run?.()
  }

  function menuKeys(event: KeyboardEvent) {
    const buttons = [
      ...(menuElement?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []),
    ]
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const step = event.key === 'ArrowDown' ? 1 : -1
      buttons[(index + step + buttons.length) % buttons.length]?.focus()
    } else if (event.key === 'ArrowLeft' && overlay.menu?.parents.length) {
      overlay.leaveSubmenu()
    } else if (event.key === 'Tab') {
      // a menu is not a place to tab through, Tab leaves it like Esc
      event.preventDefault()
      overlay.closeMenu()
    }
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape') return
    if (overlay.menu) overlay.closeMenu()
    else if (overlay.dialog) overlay.answer(null)
  }
</script>

<svelte:window
  {oncontextmenu}
  {onkeydown}
  onblur={() => overlay.closeMenu()}
  onresize={() => overlay.closeMenu()}
/>

{#if overlay.menu}
  <div
    class="menu-catcher"
    class:dim={overlay.menu.sheet}
    role="presentation"
    onpointerdown={() => overlay.closeMenu()}
    oncontextmenu={(e) => {
      e.preventDefault()
      overlay.closeMenu()
    }}
  ></div>
  <div
    class="menu"
    class:sheet={overlay.menu.sheet}
    role="menu"
    tabindex="-1"
    bind:this={menuElement}
    style:left={overlay.menu.sheet ? null : `${position.left}px`}
    style:top={overlay.menu.sheet ? null : `${position.top}px`}
    onkeydown={menuKeys}
  >
    {#if overlay.menu.parents.length > 0}
      <button class="item" role="menuitem" onclick={() => overlay.leaveSubmenu()}>
        <Icon name="back" size={16} />{t('common.back')}
      </button>
      <hr />
    {/if}
    {#each overlay.menu.items as item, i (i)}
      {#if item.separator}
        <hr />
      {:else}
        <button
          class="item"
          class:danger={item.danger}
          role={item.checked === undefined ? 'menuitem' : 'menuitemradio'}
          aria-checked={item.checked}
          disabled={item.disabled}
          onclick={() => choose(item)}
        >
          {#if item.icon}<Icon name={item.icon} size={16} />{:else}<span class="spacer"></span>{/if}
          <span class="label">{item.label}</span>
          {#if item.checked}<Icon name="check" size={14} />{/if}
          {#if item.items}<Icon name="right" size={14} />{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

{#if overlay.dialog}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && overlay.answer(null)}
  >
    <!-- Esc is answered by onkeydown above -->
    <div role="alertdialog" aria-modal="true" aria-label={overlay.dialog.message} use:modal>
      <form
        class="dialog"
        onsubmit={(e) => {
          e.preventDefault()
          overlay.answer(overlay.dialog?.kind === 'text' ? draft : true)
        }}
      >
        <p>{overlay.dialog.message}</p>
        {#if overlay.dialog.kind === 'text'}
          <input
            class="field"
            type={overlay.dialog.secret ? 'password' : 'text'}
            autocomplete={overlay.dialog.digits
              ? 'off'
              : overlay.dialog.secret
                ? 'current-password'
                : 'off'}
            inputmode={overlay.dialog.digits ? 'numeric' : undefined}
            maxlength={overlay.dialog.digits ? PIN_LENGTH : undefined}
            bind:this={textInput}
            bind:value={draft}
          />
        {/if}
        <div class="actions">
          <!-- a dangerous question starts on Cancel, Enter by habit keeps everything -->
          <button
            class="btn"
            type="button"
            data-autofocus={overlay.dialog.kind === 'ask' && overlay.dialog.danger ? '' : undefined}
            onclick={() => overlay.answer(null)}>{t('common.cancel')}</button
          >
          <button
            class="btn primary"
            class:danger={overlay.dialog.kind === 'ask' && overlay.dialog.danger}
            type="submit"
            data-autofocus={overlay.dialog.kind === 'ask' && !overlay.dialog.danger
              ? ''
              : undefined}
            >{overlay.dialog.kind === 'ask' ? overlay.dialog.confirmLabel : t('common.ok')}</button
          >
        </div>
      </form>
    </div>
  </div>
{/if}

<style>
  .menu-catcher {
    position: fixed;
    inset: 0;
    z-index: 60;
  }
  .menu-catcher.dim {
    background: rgb(0 0 0 / 0.35);
  }
  /* a long touch: the same items along the bottom, with room for a finger */
  .menu.sheet {
    inset: auto 0 0 0;
    max-width: none;
    max-height: 70vh;
    padding: var(--space-2) var(--space-2) calc(var(--space-2) + env(safe-area-inset-bottom));
    border-width: 1px 0 0;
    border-radius: var(--radius-popover) var(--radius-popover) 0 0;
  }
  .menu.sheet .item {
    min-height: 48px;
    font-size: 16px;
  }
  .menu {
    position: fixed;
    z-index: 61;
    min-width: 200px;
    max-width: min(320px, calc(100vw - 8px));
    max-height: calc(100vh - 8px);
    overflow: auto;
    padding: var(--space-1);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.35);
  }
  .item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 6px var(--space-2);
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: pointer;
  }
  .item:focus-visible {
    background: var(--color-selection);
    outline: none;
  }
  @media (hover: hover) {
    .item:hover:not(:disabled) {
      background: var(--color-selection);
    }
  }
  @media (pointer: coarse) {
    .item {
      min-height: 48px;
      font-size: 16px;
    }
  }
  .item:disabled {
    color: var(--color-text-faint);
    cursor: default;
  }
  .item.danger {
    color: var(--color-danger);
  }
  .label {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .spacer {
    width: 16px;
    flex: none;
  }
  hr {
    margin: var(--space-1) 0;
    border: 0;
    border-top: 1px solid var(--color-border);
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: rgb(0 0 0 / 0.5);
  }
  [role='alertdialog'] {
    width: min(420px, 100%);
  }
  .dialog {
    display: grid;
    gap: var(--space-4);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  p {
    margin: 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
