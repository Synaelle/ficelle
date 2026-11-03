<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- About: which versions come (Stable or Beta, not on a Mac), how they
     come, and a check right now. Only release builds update themselves, the
     others show nothing. The phone settings show the channel as rows of
     their own and leave it out here (channel={false}). -->
<script lang="ts">
  import { channelSwitch } from '../lib/channelChange.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { appUpdates, channelChoice, updatesHere, waitingForStable } from '../lib/appUpdate.svelte'
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'

  let { channel = true }: { channel?: boolean } = $props()

  const phone = !!window.AppInstaller
  const CHANNELS = ['stable', 'beta'] as const

  /** Turning the security checks off waits a few seconds on a warning. */
  async function setSecurityChecks(box: HTMLInputElement) {
    if (box.checked) {
      prefs.securityChecks = true
      return
    }
    box.checked = true
    const yes = await overlay.ask(t('appUpdate.securityOffWarning'), {
      confirmLabel: t('appUpdate.securityOff'),
      danger: true,
      wait: 5,
    })
    if (yes) prefs.securityChecks = false
  }
</script>

{#if updatesHere}
  <div class="updates">
    {#if channel && channelChoice}
      <fieldset class="channel">
        <legend>{t('appUpdate.channel')}</legend>
        <div class="segmented">
          {#each CHANNELS as choice (choice)}
            <label class:checked={prefs.channel === choice}>
              <input
                type="radio"
                name="channel"
                value={choice}
                checked={prefs.channel === choice}
                onchange={() => void channelSwitch.choose(choice)}
              />{choice === 'stable' ? t('appUpdate.channelStable') : t('appUpdate.channelBeta')}
            </label>
          {/each}
        </div>
        <p class="muted">
          {prefs.channel === 'stable'
            ? t('appUpdate.channelStableHelp')
            : t('appUpdate.channelBetaHelp')}
        </p>
      </fieldset>
    {/if}
    {#if waitingForStable() && !appUpdates.ready}
      <p class="muted">{t('appUpdate.channelWaiting')}</p>
    {/if}
    <label class="settings-check">
      <input type="checkbox" bind:checked={prefs.autoUpdates} />
      {t('appUpdate.auto')}
    </label>
    {#if !prefs.autoUpdates}
      <label class="settings-check">
        <input
          type="checkbox"
          checked={prefs.securityChecks}
          onchange={(e) => void setSecurityChecks(e.currentTarget)}
        />
        {t('appUpdate.securityChecks')}
      </label>
    {/if}
    <!-- one line: how updates come while on, what is still looked for when off -->
    <p class="muted">
      {!prefs.autoUpdates
        ? prefs.securityChecks
          ? t('appUpdate.autoOffHelp')
          : t('appUpdate.noChecksHelp')
        : phone
          ? t('appUpdate.helpPhone')
          : __APP_PORTABLE__
            ? t('appUpdate.helpPortable')
            : t('appUpdate.help')}
    </p>
    {#if appUpdates.ready?.page}
      <p>{t('appUpdate.out', { version: appUpdates.ready.version })}</p>
      <button class="btn primary" onclick={() => void channelSwitch.update()}
        >{t('appUpdate.download')}</button
      >
    {:else if appUpdates.ready}
      <p>{t('appUpdate.ready', { version: appUpdates.ready.version })}</p>
      <button class="btn primary" onclick={() => void channelSwitch.update()}
        >{phone ? t('appUpdate.install') : t('appUpdate.restart')}</button
      >
    {:else}
      <button class="btn" disabled={appUpdates.checking} onclick={() => void appUpdates.check()}
        >{appUpdates.checking ? t('appUpdate.checking') : t('appUpdate.check')}</button
      >
      {#if appUpdates.upToDate}<p class="muted" role="status">{t('appUpdate.upToDate')}</p>{/if}
    {/if}
    {#if appUpdates.error}<p class="error" role="alert">{t(`errors.${appUpdates.error}`)}</p>{/if}
  </div>
{/if}

<style>
  .updates {
    display: grid;
    justify-items: start;
    gap: var(--space-2);
  }
  p {
    margin: 0;
  }
  .channel {
    display: grid;
    justify-items: start;
    gap: var(--space-2);
    margin: 0 0 var(--space-2);
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: var(--space-2);
    padding: 0;
  }
  input[type='radio'] {
    position: absolute;
    opacity: 0;
  }
  .segmented {
    display: inline-flex;
    gap: var(--space-1);
  }
  .segmented label {
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    cursor: pointer;
  }
  .segmented .checked {
    border-color: var(--color-accent);
  }
  .segmented label:has(:focus-visible) {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .error {
    color: var(--color-danger);
  }
</style>
