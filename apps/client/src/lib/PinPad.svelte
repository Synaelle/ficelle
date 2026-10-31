<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A PIN: one dot per digit and a number pad. Typing works too (a hidden
     field, the number keyboard on phones). The last digit sends it, and
     the dots clear for the next try. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { PIN_LENGTH } from './backend'
  import { t } from './i18n.svelte'
  import Icon from './icons/Icon.svelte'

  let {
    label,
    disabled = false,
    shake = 0,
    onpin,
  }: {
    /** What the field is for, read out by screen readers. */
    label: string
    disabled?: boolean
    /** Changes when the PIN was wrong: the dots shake. */
    shake?: number
    onpin: (pin: string) => void
  } = $props()

  let pin = $state('')
  let field: HTMLInputElement | undefined = $state()
  let shaking = $state(false)

  onMount(() => field?.focus())

  $effect(() => {
    if (!shake) return
    shaking = true
    const timer = setTimeout(() => (shaking = false), 400)
    return () => clearTimeout(timer)
  })

  function set(value: string) {
    pin = value.replace(/\D/g, '').slice(0, PIN_LENGTH)
    if (pin.length === PIN_LENGTH) {
      const done = pin
      pin = ''
      onpin(done)
    }
  }

  function press(digit: string) {
    if (disabled) return
    set(pin + digit)
    field?.focus()
  }

  function erase() {
    pin = pin.slice(0, -1)
    field?.focus()
  }
</script>

<div class="pinpad">
  <label class="dots" class:shaking>
    <span class="sr-only">{label}</span>
    <input
      bind:this={field}
      class="hidden-field"
      type="password"
      inputmode="numeric"
      autocomplete="off"
      maxlength={PIN_LENGTH}
      {disabled}
      value={pin}
      oninput={(e) => set(e.currentTarget.value)}
    />
    {#each { length: PIN_LENGTH }, i (i)}
      <span class="dot" class:filled={i < pin.length}></span>
    {/each}
  </label>
  <div class="keys">
    {#each ['1', '2', '3', '4', '5', '6', '7', '8', '9'] as digit (digit)}
      <button type="button" class="key" {disabled} onclick={() => press(digit)}>{digit}</button>
    {/each}
    <span></span>
    <button type="button" class="key" {disabled} onclick={() => press('0')}>0</button>
    <button
      type="button"
      class="key quiet"
      disabled={disabled || !pin}
      aria-label={t('pin.erase')}
      onclick={erase}><Icon name="backspace" size={20} /></button
    >
  </div>
</div>

<style>
  .pinpad {
    display: grid;
    justify-items: center;
    gap: var(--space-3);
  }
  .dots {
    position: relative;
    display: flex;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-popover);
    cursor: text;
  }
  .dots:focus-within {
    outline: 2px solid color-mix(in srgb, var(--color-accent) 50%, transparent);
  }
  .hidden-field {
    position: absolute;
    inset: 0;
    opacity: 0;
    width: 100%;
    border: 0;
    cursor: text;
  }
  .dot {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid var(--color-border-strong);
    transition:
      background var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out);
  }
  .dot.filled {
    background: var(--color-accent);
    border-color: var(--color-accent);
  }
  .shaking {
    animation: shake 360ms ease-in-out;
  }
  @keyframes shake {
    20%,
    60% {
      transform: translateX(-6px);
    }
    40%,
    80% {
      transform: translateX(6px);
    }
  }
  .keys {
    display: grid;
    grid-template-columns: repeat(3, 64px);
    gap: var(--space-2) var(--space-4);
  }
  .key {
    display: grid;
    place-items: center;
    width: 64px;
    height: 48px;
    border: 0;
    border-radius: var(--radius-popover);
    background: color-mix(in srgb, var(--color-text) 6%, transparent);
    color: var(--color-text);
    font: inherit;
    font-size: 22px;
    cursor: pointer;
  }
  .key:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-text) 11%, transparent);
  }
  .key:active:not(:disabled) {
    background: color-mix(in srgb, var(--color-accent) 25%, transparent);
  }
  .key.quiet {
    background: none;
    color: var(--color-text-muted);
  }
  .key:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  @media (prefers-reduced-motion: reduce) {
    .shaking {
      animation: none;
    }
  }
</style>
