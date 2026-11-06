<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Asked once, right after the notes open: Stable or Beta updates. Stable
     is chosen until the person picks Beta. Never on the lock screen, never
     on a Mac (always Beta). About changes it later. -->
<script lang="ts">
  import { channelSwitch } from './channelChange.svelte'
  import type { UpdateChannel } from './backend'
  import Icon from './icons/Icon.svelte'
  import { t } from './i18n.svelte'
  import { modal } from './modal'
  import { prefs } from './prefs.svelte'

  let chosen: UpdateChannel = $state(prefs.channel)

  const choices = [
    {
      id: 'stable',
      icon: 'shield',
      title: () => t('appUpdate.channelStable'),
      help: () => t('appUpdate.channelStableHelp'),
    },
    {
      id: 'beta',
      icon: 'flask',
      title: () => t('appUpdate.channelBeta'),
      help: () => t('appUpdate.channelBetaHelp'),
    },
  ] as const

  /** Asked once, even when the step after it is cancelled. */
  function done() {
    prefs.channelAsked = true
    void channelSwitch.choose(chosen)
  }
</script>

<div class="backdrop" role="presentation">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="channel-title"
    aria-describedby="channel-help"
    use:modal={{ onclose: done }}
  >
    <form
      onsubmit={(e) => {
        e.preventDefault()
        done()
      }}
    >
      <h2 id="channel-title">{t('appUpdate.channelAskTitle')}</h2>
      <div class="choices" role="radiogroup" aria-labelledby="channel-title">
        {#each choices as choice (choice.id)}
          <label class="choice" class:chosen={chosen === choice.id}>
            <input type="radio" name="channel" value={choice.id} bind:group={chosen} />
            <span class="tile"><Icon name={choice.icon} size={18} /></span>
            <span class="words">
              <strong>{choice.title()}</strong>
              <span class="muted small">{choice.help()}</span>
            </span>
          </label>
        {/each}
      </div>
      <p id="channel-help" class="muted small">{t('appUpdate.channelAskHelp')}</p>
      <div class="actions">
        <button class="btn primary" type="submit">{t('appUpdate.channelAskDone')}</button>
      </div>
    </form>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    width: min(440px, 100%);
    max-height: calc(100dvh - 2 * var(--space-4));
    overflow-y: auto;
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  form {
    display: grid;
    gap: var(--space-3);
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  p {
    margin: 0;
  }
  .small {
    font-size: 13px;
  }
  .choices {
    display: grid;
    gap: var(--space-2);
  }
  .choice {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 44px;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    cursor: pointer;
  }
  .choice.chosen {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 8%, transparent);
  }
  .choice input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }
  .choice:has(input:focus-visible) {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .tile {
    display: grid;
    place-items: center;
    flex: none;
    width: 36px;
    height: 36px;
    border-radius: var(--radius-control);
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-accent-text);
  }
  .words {
    display: grid;
    gap: 2px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  .actions .btn {
    min-width: 96px;
    justify-content: center;
  }
  @media (pointer: coarse) {
    .actions .btn {
      flex: 1;
      min-height: 44px;
    }
  }
</style>
