<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Light or dark, accent, corners and fonts in the desktop settings, every
     choice applies at once. Groups of rows like the phone's (ui/). The
     theme choice comes back with the themes page. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import FontPicker from './FontPicker.svelte'
  import AccentPicker from './AccentPicker.svelte'
  import CornersPicker from './CornersPicker.svelte'
  import LayoutPicker from './LayoutPicker.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Switch from './ui/Switch.svelte'

  /** Experimental keeps the layout choice apart. */
  let { layout = true }: { layout?: boolean } = $props()
</script>

<Group>
  <Row label={t('settings.mode')}>
    <div class="segmented" role="radiogroup" aria-label={t('settings.mode')}>
      {#each ['system', 'dark', 'light'] as const as mode (mode)}
        <label class:checked={prefs.mode === mode}>
          <input type="radio" name="mode" value={mode} bind:group={prefs.mode} />{t(
            `settings.mode_${mode}`,
          )}
        </label>
      {/each}
    </div>
  </Row>
  <Row label={t('settings.pureBlack')}>
    <Switch label={t('settings.pureBlack')} bind:checked={prefs.pureBlack} />
  </Row>
  <Row label={t('settings.warmLight')}>
    <Switch label={t('settings.warmLight')} bind:checked={prefs.warmLight} />
  </Row>
</Group>

<Group title={t('settings.accent')}>
  <div class="inside"><AccentPicker /></div>
</Group>

<Group title={t('settings.corners')} note={t('settings.cornersHelp')}>
  <div class="inside"><CornersPicker /></div>
</Group>

<!-- open, as on the phone; side by side when there is room -->
<Group title={t('fonts.title')}>
  <div class="inside fonts">
    <FontPicker role="notes" />
    <FontPicker role="code" />
  </div>
</Group>

{#if layout}<LayoutPicker />{/if}

<style>
  .inside {
    display: grid;
    justify-items: start;
    gap: var(--space-2);
    padding: var(--space-3);
  }
  .fonts {
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    align-items: start;
    justify-items: stretch;
  }
</style>
