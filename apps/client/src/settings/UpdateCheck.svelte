<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- About: which versions come (Stable or Beta, not on a Mac), how they
     come, and a check right now. Only release builds update themselves, the
     others show nothing. The phone settings show it as groups of rows and
     the channel as rows of their own (rows). -->
<script lang="ts">
  import { ANDROID } from '../lib/platform'
  import { channelSwitch } from '../lib/channelChange.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import {
    appUpdates,
    channelChoice,
    notificationsHere,
    updatesHere,
    waitingForStable,
  } from '../lib/appUpdate.svelte'
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'

  let { rows = false }: { rows?: boolean } = $props()

  const phone = ANDROID
  const CHANNELS = ['stable', 'beta'] as const

  /** Turning the security checks off waits a few seconds on a warning. */
  async function securityOff() {
    const yes = await overlay.ask(t('appUpdate.securityOffWarning'), {
      confirmLabel: t('appUpdate.securityOff'),
      danger: true,
      wait: 5,
    })
    if (yes) prefs.securityChecks = false
  }

  async function setSecurityChecks(box: HTMLInputElement) {
    if (box.checked) {
      prefs.securityChecks = true
      return
    }
    box.checked = true
    await securityOff()
  }

  /** The system refused notifications: says where to allow them. */
  let notifyDenied = $state(false)

  async function setNotifications(on: boolean, box?: HTMLInputElement) {
    notifyDenied = !(await appUpdates.setNotifications(on))
    // refused, the setting stays as it was and so does the box
    if (box) box.checked = prefs.updateNotifications
  }

  /** One line: how updates come while on, what is still looked for when off. */
  const help = $derived(
    !prefs.autoUpdates
      ? prefs.securityChecks
        ? t('appUpdate.autoOffHelp')
        : t('appUpdate.noChecksHelp')
      : phone
        ? t('appUpdate.helpPhone')
        : __APP_PORTABLE__
          ? t('appUpdate.helpPortable')
          : t('appUpdate.help'),
  )
</script>

{#if updatesHere && rows}
  <Group title={t('appUpdate.title')} note={help}>
    <Row
      label={t('appUpdate.auto')}
      on={prefs.autoUpdates}
      onclick={() => (prefs.autoUpdates = !prefs.autoUpdates)}
    />
    {#if !prefs.autoUpdates}
      <Row
        label={t('appUpdate.securityChecks')}
        on={prefs.securityChecks}
        onclick={() => {
          if (prefs.securityChecks) void securityOff()
          else prefs.securityChecks = true
        }}
      />
    {/if}
    {#if notificationsHere}
      <Row
        label={t('appUpdate.notify')}
        help={notifyDenied ? t('appUpdate.notifyDenied') : undefined}
        on={prefs.updateNotifications}
        onclick={() => void setNotifications(!prefs.updateNotifications)}
      />
    {/if}
    {#if appUpdates.ready}
      <Row
        label={appUpdates.ready.page
          ? t('appUpdate.out', { version: appUpdates.ready.version })
          : t('appUpdate.ready', { version: appUpdates.ready.version })}
      >
        <button class="btn primary" onclick={() => void channelSwitch.update()}
          >{appUpdates.ready.page
            ? t('appUpdate.download')
            : phone
              ? t('appUpdate.install')
              : t('appUpdate.restart')}</button
        >
      </Row>
    {:else}
      <Row
        label={appUpdates.checking ? t('appUpdate.checking') : t('appUpdate.check')}
        help={appUpdates.upToDate
          ? t('appUpdate.upToDate')
          : waitingForStable()
            ? t('appUpdate.channelWaiting')
            : undefined}
        disabled={appUpdates.checking}
        onclick={() => void appUpdates.check()}
      />
    {/if}
    {#if appUpdates.error}<p class="error inside" role="alert">
        {t(`errors.${appUpdates.error}`)}
      </p>{/if}
  </Group>
{:else if updatesHere}
  <div class="updates">
    {#if channelChoice}
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
    {#if notificationsHere}
      <label class="settings-check">
        <input
          type="checkbox"
          checked={prefs.updateNotifications}
          onchange={(e) => void setNotifications(e.currentTarget.checked, e.currentTarget)}
        />
        {t('appUpdate.notify')}
      </label>
      {#if notifyDenied}<p class="settings-help" role="status">
          {t('appUpdate.notifyDenied')}
        </p>{/if}
    {/if}
    <p class="settings-help">{help}</p>
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
  .inside {
    padding: 0 var(--space-3) var(--space-2);
  }
</style>
