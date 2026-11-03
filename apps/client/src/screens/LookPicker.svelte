<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Dark, pure black, light or warm paper, each a small window drawn with
     that palette's own colors, or the system's choice under them. One pick
     sets the mode and both variants, and shows on the whole app at once.
     The first-run setup uses it, the settings keep their own pickers. -->
<script lang="ts">
  import { modePalette, type Mode } from '@app/ui/theme'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import { currentTheme } from '../lib/theme.svelte'

  type Look = 'dark' | 'black' | 'light' | 'warm' | 'system'

  const LOOKS: { look: Look; mode: Mode; black: boolean; warm: boolean; label: () => string }[] = [
    { look: 'dark', mode: 'dark', black: false, warm: false, label: () => t('setup.lookDark') },
    { look: 'black', mode: 'dark', black: true, warm: false, label: () => t('settings.pureBlack') },
    {
      look: 'light',
      mode: 'light',
      black: false,
      warm: false,
      label: () => t('settings.mode_light'),
    },
    { look: 'warm', mode: 'light', black: false, warm: true, label: () => t('settings.warmLight') },
  ]

  const chosen: Look = $derived(
    prefs.mode === 'system'
      ? 'system'
      : prefs.mode === 'dark'
        ? prefs.pureBlack
          ? 'black'
          : 'dark'
        : prefs.warmLight
          ? 'warm'
          : 'light',
  )

  function choose(look: Look) {
    const found = LOOKS.find((l) => l.look === look)
    prefs.mode = found ? found.mode : 'system'
    prefs.pureBlack = found?.black ?? false
    prefs.warmLight = found?.warm ?? false
  }

  /** The palette's colors as the preview's own variables. */
  function colors(mode: Mode, black: boolean, warm: boolean): string {
    const p = modePalette(currentTheme(), mode, black, warm)
    return [
      `--p-bg:${p.bg}`,
      `--p-surface:${p.surface}`,
      `--p-border:${p.border}`,
      `--p-text:${p.text}`,
      `--p-muted:${p.textMuted}`,
      `--p-accent:${p.accent}`,
    ].join(';')
  }
</script>

<div class="looks" role="radiogroup" aria-label={t('setup.lookTitle')}>
  <div class="grid">
    {#each LOOKS as item (item.look)}
      <label class="look" class:chosen={chosen === item.look}>
        <input
          type="radio"
          name="look"
          value={item.look}
          checked={chosen === item.look}
          onchange={() => choose(item.look)}
        />
        <span class="preview" style={colors(item.mode, item.black, item.warm)} aria-hidden="true">
          <span class="side"></span>
          <span class="line wide"></span>
          <span class="line"></span>
          <span class="line short"></span>
          <span class="dot"></span>
        </span>
        <span class="name">{item.label()}</span>
        {#if chosen === item.look}
          <span class="check" aria-hidden="true"><Icon name="check" size={14} /></span>
        {/if}
      </label>
    {/each}
  </div>
  <label class="look row" class:chosen={chosen === 'system'}>
    <input
      type="radio"
      name="look"
      value="system"
      checked={chosen === 'system'}
      onchange={() => choose('system')}
    />
    <span class="split" aria-hidden="true">
      <span style={colors('dark', false, false)}></span>
      <span style={colors('light', false, false)}></span>
    </span>
    <span class="words">
      <span class="name">{t('setup.lookSystem')}</span>
      <span class="muted small">{t('setup.lookSystemHelp')}</span>
    </span>
    {#if chosen === 'system'}
      <span class="check" aria-hidden="true"><Icon name="check" size={14} /></span>
    {/if}
  </label>
</div>

<style>
  .looks {
    display: grid;
    gap: var(--space-2);
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2);
  }
  .look {
    position: relative;
    display: grid;
    justify-items: center;
    gap: var(--space-2);
    min-height: 44px;
    padding: var(--space-3) var(--space-2) var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    cursor: pointer;
  }
  /* the ring and the check say which one, not the color alone */
  .look.chosen {
    border-color: var(--color-accent);
    box-shadow: 0 0 0 1px var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 8%, transparent);
  }
  .look input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }
  .look:has(input:focus-visible) {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .name {
    font-size: 14px;
    text-align: center;
  }
  .chosen .name {
    font-weight: 600;
  }
  .check {
    position: absolute;
    top: 6px;
    right: 6px;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 999px;
    background: var(--color-accent);
    color: var(--color-accent-on);
  }
  /* a window with the note list beside a note: a few lines and the accent */
  .preview {
    position: relative;
    width: 100%;
    max-width: 150px;
    aspect-ratio: 3 / 2;
    border: 1px solid var(--p-border);
    border-radius: var(--radius-control);
    background: var(--p-bg);
    overflow: hidden;
  }
  .preview > span {
    position: absolute;
  }
  .side {
    inset: 0 auto 0 0;
    width: 30%;
    background: var(--p-surface);
    border-right: 1px solid var(--p-border);
  }
  .line {
    left: 38%;
    height: 5px;
    border-radius: 3px;
    background: var(--p-muted);
  }
  .line.wide {
    top: 18%;
    width: 46%;
    height: 7px;
    background: var(--p-text);
  }
  .line:not(.wide):not(.short) {
    top: 40%;
    width: 50%;
  }
  .line.short {
    top: 56%;
    width: 34%;
  }
  .dot {
    right: 10%;
    bottom: 12%;
    width: 18%;
    height: 14%;
    border-radius: 3px;
    background: var(--p-accent);
  }
  .row {
    display: flex;
    justify-items: start;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
  }
  .row .name {
    text-align: start;
  }
  .row .check {
    position: static;
    margin-inline-start: auto;
    flex: none;
  }
  /* half dark, half light */
  .split {
    display: flex;
    flex: none;
    width: 36px;
    height: 24px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-control);
    overflow: hidden;
  }
  .split > span {
    flex: 1;
    background: var(--p-bg);
  }
  .words {
    display: grid;
    gap: 2px;
  }
  .small {
    font-size: 13px;
  }
</style>
