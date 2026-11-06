<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The accent: the system's own (Windows, KDE, GNOME, Android 12 and up),
     the theme's, one of the circles, or any color from the last circle.
     The same on the computer and the phone. -->
<script lang="ts">
  import Icon from '../lib/icons/Icon.svelte'
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import { systemAccent } from '../lib/systemAccent.svelte'
  import { currentTheme } from '../lib/theme.svelte'
  import { ACCENTS } from './appearance'
  import Row from './ui/Row.svelte'

  /** Inside a settings group, on the phone: the system's accent as a row. */
  let { row = false }: { row?: boolean } = $props()

  /** The system's accent is in use: the circles wait, unless it gives none. */
  const systemLocked = $derived(prefs.systemAccent && (!systemAccent.read || !!systemAccent.color))
  /** A color of the person's own, not one of the circles. */
  const custom = $derived(prefs.accent !== null && !ACCENTS.some((a) => a.color === prefs.accent))
  /** The mark on a chosen color stays readable, dark on light colors. */
  const markDark = $derived.by(() => {
    const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(custom ? (prefs.accent ?? '') : '')
    if (!m) return false
    const [r = 0, g = 0, b = 0] = m.slice(1, 4).map((h = '') => parseInt(h, 16))
    return 0.299 * r + 0.587 * g + 0.114 * b > 150
  })
</script>

{#if row}
  <Row
    label={t('settings.accentSystem')}
    help={prefs.systemAccent && systemAccent.read && !systemAccent.color
      ? t('settings.accentSystemNone')
      : undefined}
    on={prefs.systemAccent}
    onclick={() => (prefs.systemAccent = !prefs.systemAccent)}
  />
{:else}
  <label class="settings-check">
    <input type="checkbox" bind:checked={prefs.systemAccent} />
    {t('settings.accentSystem')}
  </label>
  {#if prefs.systemAccent && systemAccent.read && !systemAccent.color}
    <p class="settings-help">{t('settings.accentSystemNone')}</p>
  {/if}
{/if}
<div class="accents" class:off={systemLocked} class:inside={row}>
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
  <!-- a dashed circle with a plus says any color: the system's picker opens
       from it, and the chosen color fills it -->
  <label
    class="accent own"
    class:checked={custom}
    class:filled={custom}
    class:dark={markDark}
    title={t('settings.accentCustom')}
    style:--swatch={custom ? prefs.accent : 'var(--color-surface)'}
  >
    <Icon name="plus" size={16} />
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
    border-radius: var(--radius-chip);
    background: var(--swatch);
    box-shadow: 0 0 0 1px var(--color-border);
  }
  @media (pointer: coarse) {
    .accent {
      width: 44px;
      height: 44px;
    }
  }
  /* every circle on one line across the group. A big text size zooms the page,
     so a narrower circle here is still a finger's size on the screen */
  .accents.inside {
    flex-wrap: nowrap;
    justify-content: space-between;
    gap: 2px;
    padding: var(--space-1) var(--space-3) var(--space-3);
  }
  .accents.inside .accent {
    flex: 0 1 auto;
    min-width: 0;
    height: auto;
    aspect-ratio: 1;
  }
  .accent.checked {
    box-shadow: 0 0 0 2px var(--color-text);
  }
  .accent:disabled,
  .own:has(input:disabled) {
    cursor: not-allowed;
  }
  .accent:focus-visible,
  .own:has(input:focus-visible) {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  /* any color: a dashed ring and a plus, the chosen color fills it */
  .own {
    position: relative;
    display: grid;
    place-items: center;
    color: var(--color-text-muted);
    border-style: dashed;
    border-color: var(--color-text-muted);
    box-shadow: none;
    cursor: pointer;
  }
  .own.filled {
    border-style: solid;
    border-color: var(--color-surface);
    box-shadow: 0 0 0 1px var(--color-border);
    color: #fff;
  }
  .own.filled.dark {
    color: #000;
  }
  .own.checked {
    box-shadow: 0 0 0 2px var(--color-text);
  }
  .own input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    cursor: inherit;
  }
</style>
