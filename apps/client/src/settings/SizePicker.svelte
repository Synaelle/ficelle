<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Text and button size for the desktop settings, see lib/uiSize.ts: the
     control of its settings row. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import { UI_SIZES } from '../lib/uiSize'
</script>

<div class="segmented" role="radiogroup" aria-label={t('settings.uiSize')}>
  {#each UI_SIZES as size (size)}
    <label class:checked={prefs.uiSize === size}>
      <input type="radio" name="uiSize" value={size} bind:group={prefs.uiSize} />{t(
        `settings.uiSize_${size}`,
      )}
    </label>
  {/each}
</div>
