<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- On a phone a newer version is a small mark in the note list's header
     instead of the card over the notes: its number, how far its download is,
     then Install. It opens Settings on Updates, where the card's buttons
     are. -->
<script lang="ts">
  import { appUpdates, downloadLine } from './appUpdate.svelte'
  import Icon from './icons/Icon.svelte'
  import { currentLocale, t } from './i18n.svelte'

  let { onopen }: { onopen: () => void } = $props()

  const text = $derived.by(() => {
    const { ready, progress } = appUpdates
    if (!ready) return ''
    if (progress?.total)
      return new Intl.NumberFormat(currentLocale(), { style: 'percent' }).format(
        progress.done / progress.total,
      )
    if (appUpdates.downloaded && !ready.page) return t('appUpdate.install')
    return ready.version
  })
  const label = $derived.by(() => {
    const { ready, progress } = appUpdates
    if (!ready) return ''
    if (progress) return downloadLine(progress)
    if (appUpdates.downloaded && !ready.page)
      return t('appUpdate.ready', { version: ready.version })
    return t('appUpdate.out', { version: ready.version })
  })
</script>

{#if appUpdates.ready && appUpdates.offered}
  <button class="update-mark" aria-label={label} onclick={onopen}>
    <Icon name="export" size={16} />
    <span aria-hidden="true">{text}</span>
  </button>
{/if}

<style>
  .update-mark {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    align-self: center;
    height: 32px;
    padding: 0 var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface-2);
    color: var(--color-accent-text);
    font-size: 13px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  /* a finger gets 44 px, the mark keeps its size */
  .update-mark::before {
    content: '';
    position: absolute;
    top: 50%;
    left: 50%;
    width: max(100%, 44px);
    height: 44px;
    transform: translate(-50%, -50%);
  }
</style>
