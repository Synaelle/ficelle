<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Text and button size for the desktop settings, see lib/uiSize.ts. -->
<script lang="ts">
  import { t } from '../../lib/i18n.svelte'
  import { prefs } from '../../lib/prefs.svelte'
  import { UI_SIZES } from '../../lib/uiSize'
</script>

<fieldset class="settings-group">
  <legend>{t('settings.uiSize')}</legend>
  <div class="segmented">
    {#each UI_SIZES as size (size)}
      <label class:checked={prefs.uiSize === size}>
        <input type="radio" name="uiSize" value={size} bind:group={prefs.uiSize} />{t(
          `settings.uiSize_${size}`,
        )}
      </label>
    {/each}
  </div>
  <p class="settings-help">{t('settings.uiSizeHelp')}</p>
</fieldset>
