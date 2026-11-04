<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- An on and off switch for a settings Row, the phone's switch at desktop
     size. `label` names it for screen readers, the row shows the words. -->
<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    disabled = false,
    onchange,
    drawn = false,
  }: {
    checked?: boolean
    label?: string
    disabled?: boolean
    /** Instead of binding: the new state, the parent decides. */
    onchange?: (on: boolean) => void
    /** Only the picture: the row around it is the button (Row's `on`). */
    drawn?: boolean
  } = $props()

  function flip() {
    if (onchange) onchange(!checked)
    else checked = !checked
  }
</script>

{#if drawn}
  <span class="switch" class:on={checked} aria-hidden="true"><i></i></span>
{:else}
  <button
    class="switch"
    class:on={checked}
    role="switch"
    aria-checked={checked}
    aria-label={label}
    {disabled}
    onclick={flip}><i></i></button
  >
{/if}

<style>
  .switch {
    position: relative;
    width: 34px;
    height: 20px;
    flex: none;
    padding: 0;
    border: 0;
    border-radius: 10px;
    background: var(--color-border-strong);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out);
  }
  .switch i {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--color-text);
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .switch.on {
    background: var(--color-accent);
  }
  .switch.on i {
    transform: translateX(14px);
    /* a light knob on the accent, in both modes */
    background: color-mix(in srgb, white 94%, var(--color-accent));
  }
  .switch:disabled {
    opacity: 0.5;
    cursor: default;
  }
  /* a finger's size, and a target bigger than the picture */
  @media (pointer: coarse) {
    .switch {
      width: 44px;
      height: 26px;
      border-radius: 13px;
    }
    .switch i {
      width: 22px;
      height: 22px;
    }
    .switch.on i {
      transform: translateX(18px);
    }
    button.switch::after {
      content: '';
      position: absolute;
      inset: -10px -4px;
    }
  }
</style>
