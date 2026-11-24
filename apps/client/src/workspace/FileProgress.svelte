<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- How far a file on its way to or from the server got: a bar, and
     "12 of 340 MB". -->
<script lang="ts">
  import { bytesOf } from '../lib/format'
  import { t } from '../lib/i18n.svelte'
  import type { Moving } from '../lib/sync.svelte'

  let { file }: { file: Moving } = $props()

  const share = $derived(file.total > 0 ? Math.min(1, file.done / file.total) : 0)
  const amount = $derived(bytesOf(file.done, file.total))
</script>

<span class="progress">
  <span
    class="bar"
    role="progressbar"
    aria-label={t(file.sending ? 'files.sending' : 'files.receiving', { name: file.name })}
    aria-valuemin={0}
    aria-valuemax={file.total}
    aria-valuenow={file.done}
    aria-valuetext={amount}><span class="fill" style:width="{share * 100}%"></span></span
  >
  <span class="amount">{amount}</span>
</span>

<style>
  .progress {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .bar {
    flex: 1;
    min-width: 48px;
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--color-border);
  }
  .fill {
    display: block;
    height: 100%;
    background: var(--color-accent);
  }
  @media (prefers-reduced-motion: no-preference) {
    .fill {
      transition: width var(--dur-fast) linear;
    }
  }
  .amount {
    flex: none;
    color: var(--color-text-muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
</style>
