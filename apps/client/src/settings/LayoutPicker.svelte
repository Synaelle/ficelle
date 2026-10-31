<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Standard or Lite layout. In the settings it sits under Experimental: Lite
     is less tried and less supported. A settings group (see app.css). -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
</script>

<fieldset class="settings-group">
  <legend>{t('settings.layout')}</legend>
  <div class="segmented">
    {#each ['standard', 'lite'] as const as layout (layout)}
      <label class:checked={prefs.layout === layout}>
        <input type="radio" name="layout" value={layout} bind:group={prefs.layout} />{t(
          `settings.layout_${layout}`,
        )}
      </label>
    {/each}
  </div>
  <p class="settings-help">{t('settings.layoutHelp')}</p>
</fieldset>

<style>
  input[type='radio'] {
    position: absolute;
    opacity: 0;
  }
  .segmented {
    display: inline-flex;
    gap: var(--space-1);
  }
  .segmented label {
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    cursor: pointer;
  }
  .segmented .checked {
    border-color: var(--color-accent);
  }
</style>
