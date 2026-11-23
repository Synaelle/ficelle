<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- An error's reason and its code (lib/errors), inside the line or card
     that shows it. A plain text, like a field to fill in, shows as it is. -->
<script lang="ts">
  import type { ShownError } from './errors'
  import { t } from './i18n.svelte'

  let { error }: { error: ShownError | string } = $props()
</script>

{#if typeof error === 'string'}{error}{:else}{error.reason}
  <span class="error-code">{t('common.errorCode', { code: error.code })}</span>{/if}
