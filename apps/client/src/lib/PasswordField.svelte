<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A password input with an eye button that shows what was typed. Takes
     the input's attributes and `bind:value`; hidden again when it unmounts.
     A field repeating the password above it has no eye (`eye={false}`):
     the first one shows what both hold. -->
<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements'
  import { t } from './i18n.svelte'
  import Icon from './icons/Icon.svelte'

  let {
    value = $bindable(''),
    autofocus = false,
    eye = true,
    ...rest
  }: Omit<HTMLInputAttributes, 'type' | 'value'> & { value?: string; eye?: boolean } = $props()

  let shown = $state(false)
</script>

<span class="password" class:eyed={eye}>
  <!-- svelte-ignore a11y_autofocus -->
  <input {...rest} {autofocus} type={shown ? 'text' : 'password'} bind:value />
  {#if eye}
    <button
      type="button"
      class="eye"
      aria-label={shown ? t('common.hidePassword') : t('common.showPassword')}
      aria-pressed={shown}
      onclick={() => (shown = !shown)}><Icon name={shown ? 'eyeOff' : 'eye'} size={16} /></button
    >
  {/if}
</span>

<style>
  .password {
    position: relative;
    display: grid;
    min-width: 0;
  }
  .password :global(input) {
    width: 100%;
  }
  .eyed :global(input) {
    /* room for the eye */
    padding-right: 36px;
  }
  .eye {
    position: absolute;
    top: 50%;
    right: 4px;
    translate: 0 -50%;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control, 6px);
    background: none;
    color: var(--color-text-muted);
    cursor: pointer;
  }
  .eye:hover,
  .eye[aria-pressed='true'] {
    color: var(--color-text);
  }
  .eye:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 1px;
  }
</style>
