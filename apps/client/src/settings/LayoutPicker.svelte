<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Standard or Lite layout. In the settings it sits under Experimental: Lite
     is less tried and less supported. A group of rows (ui/). -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
</script>

<Group>
  <Row label={t('settings.layout')} help={t('settings.layoutHelp')}>
    <div class="segmented" role="radiogroup" aria-label={t('settings.layout')}>
      {#each ['standard', 'lite'] as const as layout (layout)}
        <label class:checked={prefs.layout === layout}>
          <input type="radio" name="layout" value={layout} bind:group={prefs.layout} />{t(
            `settings.layout_${layout}`,
          )}
        </label>
      {/each}
    </div>
  </Row>
</Group>
