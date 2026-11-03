<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Light or dark, accent, corners and layout pickers of the settings, every choice
     applies at once. Each is a settings group (see app.css). The theme
     choice comes back with the themes page. -->
<script lang="ts">
  import { t } from '../../lib/i18n.svelte'
  import { prefs } from '../../lib/prefs.svelte'
  import FontPicker from '../FontPicker.svelte'
  import AccentPicker from '../AccentPicker.svelte'
  import CornersPicker from '../CornersPicker.svelte'
  import LayoutPicker from './LayoutPicker.svelte'

  /** The setup shows its own layout step with pictures. */
  let { layout = true }: { layout?: boolean } = $props()
</script>

<fieldset class="settings-group">
  <legend>{t('settings.mode')}</legend>
  <div class="segmented">
    {#each ['system', 'dark', 'light'] as const as mode (mode)}
      <label class:checked={prefs.mode === mode}>
        <input type="radio" name="mode" value={mode} bind:group={prefs.mode} />{t(
          `settings.mode_${mode}`,
        )}
      </label>
    {/each}
  </div>
  <!-- a pair, one for each mode -->
  <div class="checks">
    <label class="settings-check">
      <input type="checkbox" bind:checked={prefs.pureBlack} />
      {t('settings.pureBlack')}
    </label>
    <label class="settings-check">
      <input type="checkbox" bind:checked={prefs.warmLight} />
      {t('settings.warmLight')}
    </label>
  </div>
</fieldset>

<fieldset class="settings-group">
  <legend>{t('settings.accent')}</legend>
  <AccentPicker />
</fieldset>

<fieldset class="settings-group">
  <legend>{t('settings.corners')}</legend>
  <CornersPicker />
  <p class="settings-help">{t('settings.cornersHelp')}</p>
</fieldset>

<!-- open, as on the phone; side by side when there is room -->
<fieldset class="settings-group fonts">
  <legend>{t('fonts.title')}</legend>
  <FontPicker role="notes" />
  <FontPicker role="code" />
</fieldset>

{#if layout}<LayoutPicker />{/if}

<style>
  .checks {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-5);
  }
  .fonts {
    grid-template-columns: repeat(auto-fit, minmax(200px, 320px));
    align-items: start;
  }
</style>
