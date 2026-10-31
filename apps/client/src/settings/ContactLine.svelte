<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The address to write to with a question, in About, and a copy of it:
     a mail link would need each system's way to open a mail app. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { t } from '../lib/i18n.svelte'

  let copied = $state(false)

  async function copy() {
    try {
      await navigator.clipboard.writeText(brand.contactEmail)
      copied = true
      setTimeout(() => (copied = false), 2000)
    } catch {
      // the address stays on screen to select by hand
    }
  }
</script>

<p class="contact">
  {t('settings.contact')} <strong class="address">{brand.contactEmail}</strong>
  <button class="btn small" type="button" onclick={copy}
    >{copied ? t('settings.contactCopied') : t('recovery.copy')}</button
  >
</p>

<style>
  .contact {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1) var(--space-2);
  }
  .address {
    user-select: all;
    overflow-wrap: anywhere;
  }
  .small {
    padding: 2px var(--space-2);
    font-size: 12px;
  }
</style>
