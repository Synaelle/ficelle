<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Every tag in one place: find, make, rename or move, merge, colour and
     delete. Renaming takes the tags inside along, a path moves a tag. -->
<script lang="ts">
  import { TAG_COLORS, type Tag } from '../lib/backend'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { modal } from '../lib/modal'
  import { overlay, SEPARATOR, type MenuItem } from '../lib/overlay.svelte'
  import { tagLabel } from '../lib/tagMark.svelte'
  import { tagKey, tagWithin, workspace } from '../lib/workspace.svelte'

  let { close }: { close: () => void } = $props()

  let find = $state('')
  let renaming: string | null = $state(null)
  let draft = $state('')
  let error: string | null = $state(null)

  const shown = $derived(
    workspace.tags.filter((tag) => !find.trim() || tagKey(tag.name).includes(tagKey(find.trim()))),
  )
  const depth = (tag: Tag) => tag.name.split('/').length - 1

  /** Runs a change, its failure said in the window. */
  async function attempt(change: () => Promise<unknown>) {
    error = null
    try {
      await change()
    } catch (err) {
      workspace.report(err)
      error = t(`errors.${workspace.error ?? 'unknown'}`)
    }
  }

  async function create() {
    const name = await overlay.askText(t('tags.name'), find.trim())
    if (name?.trim()) await attempt(() => workspace.createTag(name))
  }

  function startRename(tag: Tag) {
    renaming = tag.id
    draft = tag.name
  }

  async function commitRename(tag: Tag) {
    const name = draft.trim()
    renaming = null
    if (name && name !== tag.name) await attempt(() => workspace.renameTag(tag.id, name))
  }

  async function remove(tag: Tag) {
    const yes = await overlay.ask(
      t('tags.confirmDelete', { name: tag.name, count: tag.noteCount }),
      {
        confirmLabel: t('tags.delete'),
        danger: true,
      },
    )
    if (yes) await attempt(() => workspace.deleteTag(tag.id))
  }

  function menu(button: HTMLElement, tag: Tag) {
    const others = workspace.tags.filter((o) => !tagWithin(o.name, tag.name))
    const items: MenuItem[] = [
      { label: t('tags.rename'), run: () => startRename(tag) },
      {
        label: t('tags.merge'),
        disabled: others.length === 0,
        items: others.map((other) => ({
          label: tagLabel(other.name),
          run: () => void attempt(() => workspace.renameTag(tag.id, other.name)),
        })),
      },
      {
        label: t('tags.colour'),
        icon: 'palette',
        items: [
          {
            label: t('tags.noColour'),
            checked: !tag.color,
            run: () => void attempt(() => workspace.setTagColor(tag.id, null)),
          },
          ...TAG_COLORS.map((color) => ({
            label: t(`tags.colour_${color}`),
            checked: tag.color === color,
            run: () => void attempt(() => workspace.setTagColor(tag.id, color)),
          })),
        ],
      },
      SEPARATOR,
      {
        label: t('tags.deleteFrom', { count: tag.noteCount }),
        icon: 'trash',
        danger: true,
        run: () => void remove(tag),
      },
    ]
    overlay.openMenuBelow(button, items)
  }
</script>

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="tags-title"
    use:modal={{ onclose: close }}
  >
    <header>
      <h2 id="tags-title">{t('tags.title')}</h2>
      <button class="icon-btn" aria-label={t('common.close')} onclick={close}
        ><Icon name="close" /></button
      >
    </header>
    <p class="muted">{t('tags.lead')}</p>
    <div class="tools">
      <input
        class="field"
        type="search"
        placeholder={t('tags.find')}
        aria-label={t('tags.find')}
        bind:value={find}
      />
      <button class="btn primary" onclick={create}
        ><Icon name="plus" size={16} />{t('tags.new')}</button
      >
    </div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if shown.length}
      <ul class="rows">
        {#each shown as tag (tag.id)}
          <li class="row" style:padding-left="calc(var(--space-3) + {depth(tag) * 16}px)">
            <span class="tag-mark" data-color={tag.color ?? 'none'}></span>
            {#if renaming === tag.id}
              <input
                class="field name"
                {@attach (field) => field.select()}
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
                class="name"
                class:empty={tag.noteCount === 0}
                ondblclick={() => startRename(tag)}>{tagLabel(tag.name)}</button
              >
            {/if}
            <span class="count"
              >{tag.noteCount ? t('tags.notes', { count: tag.noteCount }) : t('tags.noNotes')}</span
            >
            <button
              class="icon-btn"
              aria-label={t('tags.options', { name: tag.name })}
              aria-haspopup="menu"
              onclick={(e) => menu(e.currentTarget, tag)}><Icon name="more" size={16} /></button
            >
          </li>
        {/each}
      </ul>
    {:else}
      <p class="muted empty-list">{find.trim() ? t('tags.noneFound') : t('tags.none')}</p>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    /* a window like Settings: its menus and questions open above it */
    z-index: 38;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    width: min(620px, 100%);
    max-height: calc(100vh - 2 * var(--space-4));
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 18px;
  }
  p {
    margin: 0;
    font-size: 13px;
  }
  .tools {
    display: flex;
    gap: var(--space-2);
  }
  .tools .field {
    flex: 1;
    min-width: 0;
  }
  .error {
    color: var(--color-danger);
  }
  .rows {
    margin: 0;
    padding: 0;
    list-style: none;
    overflow-y: auto;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 44px;
    padding-right: var(--space-2);
  }
  .row + .row {
    border-top: 1px solid var(--color-border);
  }
  .tag-mark[data-color='none'] {
    background: transparent;
  }
  .name {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-text);
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.empty {
    color: var(--color-text-muted);
  }
  input.name {
    padding: 0 var(--space-2);
  }
  .count {
    flex: none;
    font-size: 12px;
    color: var(--color-text-muted);
  }
  .empty-list {
    padding: var(--space-3) 0;
  }
  @media (max-width: 640px) {
    .backdrop {
      place-items: end stretch;
      padding: 0;
    }
    .dialog {
      width: 100%;
      max-height: 85vh;
      border-radius: var(--radius-window) var(--radius-window) 0 0;
    }
  }
</style>
