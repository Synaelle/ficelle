<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Light or dark, accent, corners and layout pickers of the settings, every choice
     applies at once. Each is a settings group (see app.css). The theme
     choice comes back with the themes page. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import FontPicker from './FontPicker.svelte'
  import AccentPicker from './AccentPicker.svelte'
  import CornersPicker from './CornersPicker.svelte'
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
  <label class="settings-check">
    <input type="checkbox" bind:checked={prefs.pureBlack} />
    {t('settings.pureBlack')}
  </label>
  <label class="settings-check">
    <input type="checkbox" bind:checked={prefs.warmLight} />
    {t('settings.warmLight')}
  </label>
</fieldset>

<!-- open, as on the phone -->
<fieldset class="settings-group fonts">
  <legend>{t('fonts.title')}</legend>
  <FontPicker role="notes" />
  <FontPicker role="code" />
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

{#if layout}<LayoutPicker />{/if}

<style>
  input[type='radio'] {
    position: absolute;
    opacity: 0;
  }
  .segmented .checked {
    border-color: var(--color-accent);
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
</style>
