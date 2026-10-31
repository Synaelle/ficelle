<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The accent: the system's own (Windows, KDE, GNOME, Android 12 and up),
     the theme's, one of the circles, or any color from the rainbow circle.
     The same on the computer and the phone. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import { systemAccent } from '../lib/systemAccent.svelte'
  import { currentTheme } from '../lib/theme.svelte'
  import { ACCENTS } from './appearance'

  /** The system's accent is in use: the circles wait, unless it gives none. */
  const systemLocked = $derived(prefs.systemAccent && (!systemAccent.read || !!systemAccent.color))
  /** A color of the person's own, not one of the circles. */
  const custom = $derived(prefs.accent !== null && !ACCENTS.some((a) => a.color === prefs.accent))
</script>

<label class="settings-check">
  <input type="checkbox" bind:checked={prefs.systemAccent} />
  {t('settings.accentSystem')}
</label>
{#if prefs.systemAccent && systemAccent.read && !systemAccent.color}
  <p class="settings-help">{t('settings.accentSystemNone')}</p>
{/if}
<div class="accents" class:off={systemLocked}>
  <button
    class="accent"
    class:checked={prefs.accent === null}
    aria-label={t('settings.accentTheme')}
    title={t('settings.accentTheme')}
    disabled={systemLocked}
    style:--swatch={currentTheme().colors.dark.accent}
    onclick={() => (prefs.accent = null)}
  ></button>
  {#each ACCENTS as accent (accent.id)}
    <button
      class="accent"
      class:checked={prefs.accent === accent.color}
      aria-label={t(`settings.accent_${accent.id}`)}
      title={t(`settings.accent_${accent.id}`)}
      disabled={systemLocked}
      style:--swatch={accent.color}
      onclick={() => (prefs.accent = accent.color)}
    ></button>
  {/each}
  <!-- a rainbow says any color: the system's picker opens from it, and
       the chosen color shows in its middle -->
  <label
    class="accent rainbow"
    class:checked={custom}
    title={t('settings.accentCustom')}
    style:--swatch={custom ? prefs.accent : 'transparent'}
  >
    <input
      type="color"
      aria-label={t('settings.accentCustom')}
      disabled={systemLocked}
      value={prefs.accent ?? currentTheme().colors.dark.accent}
      oninput={(e) => (prefs.accent = e.currentTarget.value.toUpperCase())}
    />
  </label>
</div>

<style>
  /* the system's accent is used: the circles wait */
  .accents.off {
    opacity: 0.35;
  }
  .accents {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .accent {
    width: 28px;
    height: 28px;
    padding: 0;
    border: 2px solid var(--color-surface);
    border-radius: 50%;
    background: var(--swatch);
    box-shadow: 0 0 0 1px var(--color-border);
  }
  @media (pointer: coarse) {
    .accent {
      width: 44px;
      height: 44px;
    }
  }
  .accent.checked {
    box-shadow: 0 0 0 2px var(--color-text);
  }
  .accent:disabled,
  .rainbow:has(input:disabled) {
    cursor: not-allowed;
  }
  .accent:focus-visible,
  .rainbow:has(input:focus-visible) {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  /* any color: a rainbow ring, the chosen color in its middle */
  .rainbow {
    position: relative;
    display: grid;
    place-items: center;
    background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
    cursor: pointer;
  }
  .rainbow::after {
    content: '';
    width: 45%;
    height: 45%;
    border-radius: 50%;
    background: var(--swatch);
  }
  .rainbow input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    cursor: inherit;
  }
</style>
