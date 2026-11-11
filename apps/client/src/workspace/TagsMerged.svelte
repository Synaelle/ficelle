<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Said once, after an update turned someone's folders into tags: what
     changed, their own lists before and now, and where the # mark is. A
     centred dialog on a computer, a sheet from the bottom on a phone. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { backend } from '../lib/backend'
  import { isBeta } from '../lib/beta.svelte'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { modal } from '../lib/modal'
  import { prefs } from '../lib/prefs.svelte'

  let open = $state(false)

  onMount(async () => {
    open = await backend.tagsMerged()
  })

  /** The picture: the same example for everyone, before and now. */
  const before = [
    { name: 'exProjects', folder: true, inner: false },
    { name: 'exGarden', folder: true, inner: true },
    { name: 'exWork', folder: true, inner: false },
    { name: 'exTodo', folder: false, inner: false },
    { name: 'exIdeas', folder: false, inner: false },
  ]
  const now = [
    { name: 'exIdeas', inner: false },
    { name: 'exProjects', inner: false },
    { name: 'exGarden', inner: true },
    { name: 'exTodo', inner: false },
    { name: 'exWork', inner: false },
  ]

  async function close() {
    open = false
    await backend.tagsMergedSeen()
  }

  function useHash() {
    prefs.tagMark = 'hash'
    void close()
  }
</script>

{#if open}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && close()}
  >
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="merged-title"
      use:modal={{ onclose: close }}
    >
      <span class="eyebrow">{t('merged.eyebrow')}</span>
      <h2 id="merged-title">{t('merged.title')}</h2>
      <div class="swap" aria-hidden="true">
        <div class="box">
          <span class="cap">{t('merged.before')}</span>
          {#each before as item (item.name)}
            <span class="item" class:inner={item.inner}>
              {#if item.folder}<Icon name="folder" size={14} />{:else}<span class="hash">#</span
                >{/if}
              {t(`merged.${item.name}`)}
            </span>
          {/each}
        </div>
        <Icon name="right" size={18} />
        <div class="box">
          <span class="cap">{t('merged.now')}</span>
          {#each now as item (item.name)}
            <span class="item" class:inner={item.inner}>
              {#if prefs.tagMark === 'hash'}<span class="hash">#</span>{:else}<Icon
                  name={item.name === 'exProjects' ? 'folderOpen' : 'folder'}
                  size={14}
                />{/if}
              {t(`merged.${item.name}`)}
            </span>
          {/each}
        </div>
      </div>
      <p>{t('merged.body')}</p>
      {#if isBeta}<p class="muted">{t('merged.beta')}</p>{/if}
      <div class="actions">
        {#if prefs.tagMark !== 'hash'}
          <button class="link" onclick={useHash}>{t('merged.hash')}</button>
        {:else}
          <span></span>
        {/if}
        <button class="btn primary" data-autofocus onclick={close}>{t('merged.ok')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    width: min(520px, 100%);
    max-height: calc(100vh - 2 * var(--space-4));
    overflow-y: auto;
    display: grid;
    gap: var(--space-4);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  .eyebrow {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--color-text-muted);
  }
  h2 {
    margin: 0;
    font-size: 22px;
    line-height: 1.25;
  }
  p {
    margin: 0;
  }
  .muted {
    font-size: 13px;
  }
  .swap {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
  }
  .box {
    display: grid;
    align-content: start;
    gap: 2px;
    min-height: 140px;
    padding: var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-bg);
    color: var(--color-text);
    font-size: 13px;
  }
  .cap {
    padding-bottom: var(--space-1);
    font-size: 11px;
    color: var(--color-text-muted);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    height: 24px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .item.inner {
    padding-left: 18px;
  }
  .item :global(.icon),
  .hash {
    flex: none;
    color: var(--color-text-muted);
  }
  .hash {
    width: 14px;
    font-family: var(--font-mono);
    text-align: center;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .link {
    padding: var(--space-2) 0;
    border: 0;
    background: none;
    color: var(--color-accent-text);
    font-size: 13px;
  }
  /* a phone: a sheet from the bottom, the buttons stacked to reach */
  @media (max-width: 640px) {
    .backdrop {
      place-items: end stretch;
      padding: 0;
    }
    .dialog {
      width: 100%;
      border-radius: var(--radius-window) var(--radius-window) 0 0;
      padding-bottom: var(--space-6);
    }
    .actions {
      flex-direction: column-reverse;
      align-items: stretch;
    }
    .actions .btn {
      height: 48px;
    }
  }
</style>
