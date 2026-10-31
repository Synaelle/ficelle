<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- What the open note asks after a sync: a merge of two rewrites of the
     same passage (compare, then keep this device's, the other's, both, or
     the merge), or an edit made elsewhere while the note was deleted here.
     Every choice keeps the rest in the note's history. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { modal } from '../lib/modal'
  import { workspace } from '../lib/workspace.svelte'

  let { narrow }: { narrow: boolean } = $props()

  const conflict = $derived(workspace.conflict)
  const note = $derived(workspace.current)
  let comparing = $state(false)
  let busy = $state(false)

  async function keep(choice: 'merged' | 'mine' | 'theirs' | 'both') {
    if (!note) return
    busy = true
    comparing = false
    const title = note.title || t('note.untitled')
    await workspace.resolve(
      choice === 'both'
        ? { keep: 'both', copyTitle: t('conflict.copyTitle', { title }) }
        : { keep: choice },
    )
    busy = false
  }

  async function restore() {
    if (!note) return
    await workspace.restore(note.id)
    await workspace.open(note.id)
  }

  const columns = $derived(
    conflict?.mine && conflict.theirs && note
      ? [
          { key: 'mine', label: t('conflict.mine'), text: conflict.mine },
          { key: 'theirs', label: t('conflict.theirs'), text: conflict.theirs },
          { key: 'merged', label: t('conflict.merged'), text: note },
        ]
      : [],
  )
</script>

{#if conflict && note}
  <div class="banner" role="status">
    <Icon name="syncError" size={18} />
    {#if conflict.kind === 'merge'}
      <p>{t('conflict.mergeText')}</p>
      <div class="buttons">
        <button class="btn primary" disabled={busy} onclick={() => (comparing = true)}
          >{t('conflict.compare')}</button
        >
        <button class="btn" disabled={busy} onclick={() => keep('merged')}
          >{t('conflict.keepMerged')}</button
        >
      </div>
    {:else}
      <p>{t('conflict.deletedText')}</p>
      <div class="buttons">
        {#if note.trashedAt}
          <button class="btn primary" onclick={restore}>{t('conflict.restore')}</button>
        {/if}
        <button class="btn" onclick={() => keep('merged')}>{t('common.ok')}</button>
      </div>
    {/if}
  </div>
{/if}

{#if comparing && conflict && note}
  <div class="backdrop" role="presentation">
    <div
      class="dialog"
      class:narrow
      role="dialog"
      aria-modal="true"
      aria-label={t('conflict.title')}
      use:modal={{ onclose: () => (comparing = false) }}
    >
      <header>
        <h2>{t('conflict.title')}</h2>
        <button class="icon-btn" aria-label={t('common.close')} onclick={() => (comparing = false)}
          ><Icon name="close" /></button
        >
      </header>
      <p class="muted">{t('conflict.help')}</p>
      <div class="columns">
        {#each columns as column (column.key)}
          <section>
            <h3>{column.label}</h3>
            <strong class="title">{column.text.title || t('note.untitled')}</strong>
            <pre>{column.text.body}</pre>
          </section>
        {/each}
      </div>
      <footer>
        <button class="btn" disabled={busy} onclick={() => keep('mine')}
          >{t('conflict.keepMine')}</button
        >
        <button class="btn" disabled={busy} onclick={() => keep('theirs')}
          >{t('conflict.keepTheirs')}</button
        >
        <button class="btn" disabled={busy} onclick={() => keep('both')}
          >{t('conflict.keepBoth')}</button
        >
        <button class="btn primary" disabled={busy} onclick={() => keep('merged')}
          >{t('conflict.keepMerged')}</button
        >
      </footer>
    </div>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
    margin-bottom: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border-strong);
    border-left: 3px solid var(--color-accent);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  .banner p {
    flex: 1 1 240px;
    margin: 0;
  }
  .buttons,
  footer {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: color-mix(in srgb, var(--color-bg) 70%, transparent);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    width: min(1100px, 100%);
    max-height: calc(100dvh - 2 * var(--space-4));
    padding: var(--space-4) var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-window);
  }
  .dialog.narrow {
    padding: var(--space-3);
  }
  header {
    display: flex;
    align-items: center;
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 17px;
  }
  .dialog > p {
    margin: 0;
  }
  .columns {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-3);
    min-height: 0;
    overflow: auto;
  }
  .narrow .columns {
    grid-template-columns: 1fr;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-bg);
  }
  h3 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
  }
  pre {
    margin: 0;
    max-height: 40vh;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: var(--font-mono);
    font-size: 13px;
  }
  footer {
    justify-content: flex-end;
  }
  /* phones: two buttons a row, all of them in view */
  .narrow footer {
    display: grid;
    grid-template-columns: 1fr 1fr;
  }
  .narrow pre {
    max-height: none;
  }
</style>
