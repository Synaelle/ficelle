<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- One setting in a Group: its name and an optional short help on the
     left, its control on the right. When both do not fit, in a long
     language or on a phone, the control moves under the name instead of
     squeezing it. A row with `onclick` is a button as a whole (a choice in
     a list, with `checked` showing the picked one). -->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import Icon from '../../lib/icons/Icon.svelte'
  import Switch from './Switch.svelte'

  let {
    label,
    help,
    checked,
    on,
    onclick,
    disabled = false,
    opens = false,
    children,
  }: {
    label: string
    help?: string
    /** A row in a list of choices: true marks the picked one. */
    checked?: boolean
    /** A row that is a switch as a whole, for fingers: its state. */
    on?: boolean
    onclick?: () => void
    /** A row button that does nothing here (the browser has no files). */
    disabled?: boolean
    /** A row that opens something else: an arrow on the right. */
    opens?: boolean
    /** The control on the right. */
    children?: Snippet
  } = $props()
</script>

{#snippet body()}
  <span class="text">
    <span class="label">{label}</span>
    {#if help}<span class="help">{help}</span>{/if}
  </span>
  {#if children}<span class="control">{@render children()}</span>{/if}
  {#if on !== undefined}<Switch drawn checked={on} />{/if}
  {#if opens}<span class="opens" aria-hidden="true"><Icon name="right" size={16} /></span>{/if}
  {#if checked !== undefined}
    <span class="check" aria-hidden="true"
      >{#if checked}<Icon name="check" size={16} />{/if}</span
    >
  {/if}
{/snippet}

{#if onclick}
  <button
    class="row choice settings-row"
    role={on !== undefined ? 'switch' : checked !== undefined ? 'radio' : undefined}
    aria-checked={on ?? checked}
    {disabled}
    {onclick}>{@render body()}</button
  >
{:else}
  <div class="row settings-row">{@render body()}</div>
{/if}

<style>
  .row {
    position: relative;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1) var(--space-3);
    width: 100%;
    min-height: 44px;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  /* hairlines between rows start after the padding, like the phone's */
  :global(.settings-row + .settings-row)::before {
    content: '';
    position: absolute;
    top: 0;
    left: var(--space-3);
    right: 0;
    border-top: 1px solid var(--color-border);
  }
  .choice {
    cursor: pointer;
  }
  .choice:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .opens {
    display: grid;
    flex: none;
    color: var(--color-text-faint);
  }
  @media (hover: hover) {
    .choice:hover:not(:disabled) {
      background: var(--color-hover);
    }
  }
  /* the name keeps room for a few words, else the control takes the next line */
  .text {
    display: grid;
    flex: 1 1 11rem;
    min-width: 0;
    gap: 1px;
  }
  .label {
    overflow-wrap: anywhere;
  }
  .help {
    color: var(--color-text-muted);
    font-size: 12px;
    line-height: 1.4;
  }
  .control {
    display: flex;
    flex: 0 1 auto;
    min-width: 0;
    max-width: 100%;
    align-items: center;
    gap: var(--space-2);
    overflow-wrap: anywhere;
  }
  .check {
    display: grid;
    width: 16px;
    flex: none;
    color: var(--color-accent-text);
  }
</style>
