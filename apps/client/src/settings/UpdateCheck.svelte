<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- About: which versions come (Stable or Beta, not on a Mac), how they
     come, and a check right now. Only release builds update themselves, the
     others show nothing. The phone settings show the channel as rows of
     their own and leave it out here (channel={false}). -->
<script lang="ts">
  import { ANDROID } from '../lib/platform'
  import { channelSwitch } from '../lib/channelChange.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { appUpdates, channelChoice, updatesHere, waitingForStable } from '../lib/appUpdate.svelte'
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'

  let { channel = true }: { channel?: boolean } = $props()

  const phone = ANDROID
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
        <p class="settings-help">
          {prefs.channel === 'stable'
            ? t('appUpdate.channelStableHelp')
            : t('appUpdate.channelBetaHelp')}
        </p>
      </fieldset>
    {/if}
    {#if waitingForStable() && !appUpdates.ready}
      <p class="settings-help">{t('appUpdate.channelWaiting')}</p>
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
    <p class="settings-help">
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
      <!-- the answer beside the button -->
      <div class="settings-actions check-row">
        <button class="btn" disabled={appUpdates.checking} onclick={() => void appUpdates.check()}
          >{appUpdates.checking ? t('appUpdate.checking') : t('appUpdate.check')}</button
        >
        {#if appUpdates.upToDate}<span class="settings-help" role="status"
            >{t('appUpdate.upToDate')}</span
          >{/if}
      </div>
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
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: var(--space-2);
    padding: 0;
  }
  .check-row {
    align-items: center;
    gap: var(--space-3);
  }
  .error {
    color: var(--color-danger);
  }
</style>
