<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The sync state as an icon: a cloud with a tick, arrows turning while a
     round runs, a struck cloud offline, a warning on errors. -->
<script lang="ts">
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import type { SyncState } from '../lib/sync.svelte'

  let { state, size = 16 }: { state: SyncState; size?: number } = $props()

  const ICONS: Record<SyncState, IconName> = {
    off: 'offline',
    signedOut: 'syncError',
    syncing: 'syncing',
    offline: 'offline',
    error: 'syncError',
    pending: 'syncing',
    synced: 'synced',
  }
</script>

<span
  class="sync-icon"
  class:spin={state === 'syncing'}
  class:alert={state === 'error' || state === 'signedOut'}><Icon name={ICONS[state]} {size} /></span
>

<style>
  .sync-icon {
    display: inline-flex;
  }
  .alert {
    color: var(--color-danger);
  }
  .spin {
    animation: spin 1.2s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
