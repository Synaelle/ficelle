<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Where a downloaded font stands, under its setting: downloading, or why
     it did not come, with its code and a way to try again. Nothing once it
     is here. -->
<script lang="ts">
  import type { ShownError } from '../lib/errors'
  import ErrorText from '../lib/ErrorText.svelte'
  import { t } from '../lib/i18n.svelte'

  let {
    font,
    loading,
    error,
    onretry,
  }: { font: string; loading: boolean; error: ShownError | null; onretry: () => void } = $props()
</script>

{#if loading}
  <p class="font-load" role="status">{t('settings.fontLoading', { font })}</p>
{:else if error}
  <div class="font-load failed" role="alert">
    <p>{t('settings.fontFailed', { font })} <ErrorText {error} /></p>
    <button class="btn" onclick={onretry}>{t('settings.fontRetry')}</button>
  </div>
{/if}

<style>
  .font-load {
    margin: 0;
    color: var(--color-text-muted);
    font-size: 13px;
    line-height: 1.45;
    overflow-wrap: anywhere;
  }
  .failed {
    display: grid;
    justify-items: start;
    gap: var(--space-2);
  }
  .failed p {
    margin: 0;
    color: var(--color-danger);
  }
</style>
