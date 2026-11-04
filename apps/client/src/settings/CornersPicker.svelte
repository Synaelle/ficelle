<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Square or rounded corners, each with a small window drawn in that look
     whatever the current one. The desktop and phone settings give it its
     title, see lib/theme for how the corners apply. -->
<script lang="ts">
  import type { Corners } from '@app/ui/theme'
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'

  const CHOICES: Corners[] = ['square', 'round']
</script>

<div class="choices">
  {#each CHOICES as corners (corners)}
    <label class="choice {corners}" class:checked={prefs.corners === corners}>
      <input type="radio" name="corners" value={corners} bind:group={prefs.corners} />
      <span class="preview" aria-hidden="true">
        <span class="bar"></span>
        <span class="chip"></span>
        <span class="button"></span>
      </span>
      {t(`settings.corners_${corners}`)}
    </label>
  {/each}
</div>

<style>
  input[type='radio'] {
    position: absolute;
    opacity: 0;
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .choice {
    display: grid;
    justify-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3) var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    cursor: pointer;
  }
  /* side by side across a phone's group */
  @media (pointer: coarse) {
    .choice {
      flex: 1;
    }
  }
  .choice.checked {
    border-color: var(--color-accent);
  }
  .choice:has(input:focus-visible) {
    outline: 2px solid var(--color-accent);
    outline-offset: 1px;
  }
  /* a window with a search bar, a tag and a button, a third of their size */
  .preview {
    position: relative;
    width: 88px;
    height: 56px;
    border: 1px solid var(--color-border-strong);
    background: var(--color-bg);
  }
  .preview > span {
    position: absolute;
  }
  .bar {
    inset: 7px 7px auto;
    height: 10px;
    border: 1px solid var(--color-border);
    background: var(--color-surface);
  }
  .chip {
    left: 7px;
    bottom: 8px;
    width: 26px;
    height: 9px;
    background: var(--color-surface-2);
  }
  .button {
    right: 7px;
    bottom: 7px;
    width: 30px;
    height: 12px;
    background: var(--color-accent);
  }
  .square .preview {
    border-radius: 3px;
  }
  .square .preview > span {
    border-radius: 1px;
  }
  .round .preview {
    border-radius: 10px;
  }
  .round .bar,
  .round .button {
    border-radius: 4px;
  }
  .round .chip {
    border-radius: 999px;
  }
</style>
