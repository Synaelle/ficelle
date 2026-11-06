<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The address to write to with a question, in About, and a copy of it:
     a mail link would need each system's way to open a mail app. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { t } from '../lib/i18n.svelte'
  import Row from './ui/Row.svelte'

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

<Row label={t('settings.contact')} help={brand.contactEmail}>
  <button class="btn" type="button" onclick={copy}
    >{copied ? t('settings.contactCopied') : t('recovery.copy')}</button
  >
</Row>
