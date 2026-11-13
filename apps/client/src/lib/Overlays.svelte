<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Draws the dialogs and context menus of lib/overlay. Also replaces the
     webview menu: text fields get cut, copy and paste, the rest of the app
     shows a menu only where it has one. -->
<script lang="ts">
  import { tick } from 'svelte'
  import { PIN_LENGTH } from './backend/types'
  import { t } from './i18n.svelte'
  import Icon from './icons/Icon.svelte'
  import { modal } from './modal'
  import { overlay, textItems, type MenuItem } from './overlay.svelte'
  import { pageZoom } from './uiSize'

  let menuElement: HTMLDivElement | undefined = $state()
  let position = $state({ left: 0, top: 0 })
  /** A submenu opened beside the menu, which stays open: with a mouse or the
   * keys. A finger gets the submenu in place of the menu, with a back row. */
  let flyout: { index: number; label: string; items: MenuItem[] } | null = $state(null)
  let flyoutElement: HTMLDivElement | undefined = $state()
  let flyoutPosition = $state({ left: 0, top: 0 })
  let hoverTimer: ReturnType<typeof setTimeout> | undefined
  let textInput: HTMLInputElement | undefined = $state()
  let draft = $state('')
  /** The seconds left before a question that asks for a pause can be confirmed. */
  let waitLeft = $state(0)
  $effect(() => {
    const dialog = overlay.dialog
    // counted here, not read back from the state: the effect would rerun
    let left = dialog?.kind === 'ask' ? dialog.wait : 0
    waitLeft = left
    if (left <= 0) return
    const timer = setInterval(() => {
      left = Math.max(0, left - 1)
      waitLeft = left
      if (left === 0) clearInterval(timer)
    }, 1000)
    return () => clearInterval(timer)
  })

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

  // keep the menu inside the window. Positions are as measured, placed
  // through pageZoom
  $effect(() => {
    const menu = overlay.menu
    clearTimeout(hoverTimer)
    flyout = null
    if (!menu) return
    if (menu.sheet) {
      // opened by a finger, the first row would look already pressed: the
      // sheet holds the focus itself, keys still reach its rows with Tab
      void tick().then(() => {
        if (overlay.byTouch) menuElement?.focus()
        else menuElement?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()
      })
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

  function choose(item: MenuItem, index: number, button: HTMLElement, inFlyout: boolean) {
    if (item.items) {
      if (inFlyout || overlay.menu?.sheet || overlay.byTouch) return overlay.enterSubmenu(item)
      return openFlyout(item, index, button, true)
    }
    overlay.closeMenu()
    item.run?.()
  }

  function openFlyout(item: MenuItem, index: number, button: HTMLElement, focus: boolean) {
    clearTimeout(hoverTimer)
    const menuBox = menuElement?.getBoundingClientRect()
    if (!menuBox || !item.items) return
    const row = button.getBoundingClientRect()
    // a little over the menu's edge, its first row level with the item
    flyout = { index, label: item.label, items: item.items }
    flyoutPosition = { left: menuBox.right - 4, top: row.top - 5 }
    void tick().then(() => {
      const box = flyoutElement?.getBoundingClientRect()
      if (!box) return
      const fitsRight = menuBox.right - 4 + box.width <= window.innerWidth - 4
      flyoutPosition = {
        left: fitsRight ? menuBox.right - 4 : Math.max(4, menuBox.left + 4 - box.width),
        top: Math.max(4, Math.min(row.top - 5, window.innerHeight - box.height - 4)),
      }
      if (focus) flyoutElement?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()
    })
  }

  function closeFlyout() {
    const opener = menuElement?.querySelector<HTMLButtonElement>('[aria-expanded=true]')
    flyout = null
    opener?.focus()
  }

  /** The pointer on a row of the menu: its submenu opens, another one closes.
   * A short wait, so a pointer crossing rows on its way to a submenu keeps it. */
  function hover(event: PointerEvent, item: MenuItem, index: number) {
    if (event.pointerType !== 'mouse' || overlay.menu?.sheet) return
    clearTimeout(hoverTimer)
    if (flyout?.index === index) return
    const button = event.currentTarget as HTMLElement
    hoverTimer = setTimeout(
      () => {
        if (item.items && !item.disabled) openFlyout(item, index, button, false)
        else flyout = null
      },
      flyout ? 250 : 120,
    )
  }

  function menuKeys(event: KeyboardEvent, container: HTMLElement | undefined, inFlyout: boolean) {
    const buttons = [
      ...(container?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []),
    ]
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const step = event.key === 'ArrowDown' ? 1 : -1
      buttons[(index + step + buttons.length) % buttons.length]?.focus()
    } else if (event.key === 'ArrowRight' && !inFlyout) {
      const button = buttons[index]
      if (button?.getAttribute('aria-haspopup')) button.click()
    } else if (inFlyout && (event.key === 'ArrowLeft' || event.key === 'Escape')) {
      // back to the menu, which stays open
      event.preventDefault()
      event.stopPropagation()
      closeFlyout()
    } else if (event.key === 'ArrowLeft' && overlay.menu?.parents.length) {
      overlay.leaveSubmenu()
    } else if (event.key === 'Tab') {
      // a menu is not a place to tab through, Tab leaves it like Esc
      event.preventDefault()
      overlay.closeMenu()
    }
  }

  let lastWidth = window.innerWidth

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
  onresize={() => {
    // a phone's keyboard showing or hiding changes only the height, and
    // opening a menu can hide it: the menu stays
    if (window.innerWidth !== lastWidth) overlay.closeMenu()
    lastWidth = window.innerWidth
  }}
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
    style:left={overlay.menu.sheet ? null : `${position.left / pageZoom()}px`}
    style:top={overlay.menu.sheet ? null : `${position.top / pageZoom()}px`}
    onkeydown={(e) => menuKeys(e, menuElement, false)}
  >
    {#if overlay.menu.parents.length > 0}
      <button class="item" role="menuitem" onclick={() => overlay.leaveSubmenu()}>
        <Icon name="back" size={16} />{t('common.back')}
      </button>
      <hr />
    {/if}
    {@render entries(overlay.menu.items, false)}
  </div>
  {#if flyout}
    <div
      class="menu"
      role="menu"
      tabindex="-1"
      aria-label={flyout.label}
      bind:this={flyoutElement}
      style:left="{flyoutPosition.left / pageZoom()}px"
      style:top="{flyoutPosition.top / pageZoom()}px"
      onkeydown={(e) => menuKeys(e, flyoutElement, true)}
      onpointerenter={() => clearTimeout(hoverTimer)}
    >
      {@render entries(flyout.items, true)}
    </div>
  {/if}
{/if}

{#snippet entries(items: MenuItem[], inFlyout: boolean)}
  {#each items as item, i (i)}
    {#if item.separator}
      <hr />
    {:else}
      {@const open = !inFlyout && flyout?.index === i}
      <button
        class="item"
        class:danger={item.danger}
        class:open
        role={item.checked === undefined ? 'menuitem' : 'menuitemradio'}
        aria-checked={item.checked}
        aria-haspopup={item.items ? 'menu' : undefined}
        aria-expanded={item.items && !overlay.menu?.sheet ? open : undefined}
        disabled={item.disabled}
        onclick={(e) => choose(item, i, e.currentTarget, inFlyout)}
        onpointerenter={inFlyout ? undefined : (e) => hover(e, item, i)}
      >
        {#if item.icon}<Icon name={item.icon} size={16} />{:else}<span class="spacer"></span>{/if}
        <span class="label">{item.label}</span>
        {#if item.checked}<Icon name="check" size={14} />{/if}
        {#if item.items}<Icon name="right" size={14} />{/if}
      </button>
    {/if}
  {/each}
{/snippet}

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
          if (waitLeft > 0) return
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
          {#if overlay.dialog.kind === 'ask' && overlay.dialog.altLabel}
            <button class="btn" type="button" onclick={() => overlay.answer('alt')}
              >{overlay.dialog.altLabel}</button
            >
          {/if}
          <button
            class="btn primary"
            class:danger={overlay.dialog.kind === 'ask' && overlay.dialog.danger}
            type="submit"
            disabled={waitLeft > 0}
            data-autofocus={overlay.dialog.kind === 'ask' && !overlay.dialog.danger
              ? ''
              : undefined}
            >{overlay.dialog.kind === 'ask'
              ? overlay.dialog.confirmLabel
              : t('common.ok')}{waitLeft > 0 ? ` (${waitLeft})` : ''}</button
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
    box-shadow: var(--shadow-dialog);
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
  .item:focus-visible,
  .item.open {
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
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
