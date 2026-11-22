<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Updates, the same on desktop and phone: how versions come, the nightly
     versions and a check right now. Only listed where the app has updates.
     When a store or a package manager brings them, one line says so and the
     rows stay, greyed. -->
<script lang="ts">
  import { ANDROID } from '../lib/platform'
  import { channelSwitch } from '../lib/channelChange.svelte'
  import {
    appUpdates,
    channelChoice,
    downloadLine,
    notificationsHere,
    waitingForStable,
  } from '../lib/appUpdate.svelte'
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'

  const phone = ANDROID

  /** Nothing here to change: the switches read off, as the app does not look. */
  const elsewhere = $derived(!!appUpdates.elsewhere)

  /** The system refused notifications: says where to allow them. */
  let notifyDenied = $state(false)

  async function setNotifications(on: boolean) {
    notifyDenied = !(await appUpdates.setNotifications(on))
  }
</script>

{#if appUpdates.elsewhere}
  <p class="settings-help">
    {appUpdates.elsewhere === 'package'
      ? t('appUpdate.fromPackage')
      : t('appUpdate.fromStore', { store: appUpdates.elsewhere })}
  </p>
{/if}
<Group>
  <!-- off, nothing is looked for until the check below is pressed -->
  <Row
    label={t('appUpdate.checks')}
    icon="syncing"
    help={prefs.updateChecks || elsewhere ? undefined : t('appUpdate.checksOff')}
    on={prefs.updateChecks && !elsewhere}
    disabled={elsewhere}
    onclick={() => (prefs.updateChecks = !prefs.updateChecks)}
  />
  {#if notificationsHere}
    <Row
      label={t('appUpdate.notify')}
      icon="inbox"
      help={notifyDenied ? t('appUpdate.notifyDenied') : undefined}
      on={prefs.updateNotifications && !elsewhere}
      disabled={elsewhere}
      onclick={() => void setNotifications(!prefs.updateNotifications)}
    />
  {/if}
  {#if phone}
    <Row
      label={t('appUpdate.mobileData')}
      icon="globe"
      on={prefs.updatesOnMobileData && !elsewhere}
      disabled={elsewhere}
      onclick={() => (prefs.updatesOnMobileData = !prefs.updatesOnMobileData)}
    />
  {/if}
  {#if channelChoice}
    <Row
      label={t('appUpdate.nightlyVersions')}
      icon="flask"
      on={prefs.channel === 'nightly' && !elsewhere}
      disabled={elsewhere}
      onclick={() => void channelSwitch.choose(prefs.channel === 'nightly' ? 'stable' : 'nightly')}
    />
  {/if}
  {#if appUpdates.ready}
    <Row
      label={appUpdates.downloaded
        ? t('appUpdate.ready', { version: appUpdates.ready.version })
        : t('appUpdate.out', { version: appUpdates.ready.version })}
      icon="export"
      help={appUpdates.progress ? downloadLine(appUpdates.progress) : undefined}
    >
      <button
        class="btn primary"
        disabled={!!appUpdates.progress}
        onclick={() => void channelSwitch.update()}
        >{appUpdates.ready.page || !appUpdates.downloaded
          ? t('appUpdate.download')
          : phone
            ? t('appUpdate.install')
            : t('appUpdate.restart')}</button
      >
    </Row>
  {:else}
    <Row
      label={appUpdates.checking ? t('appUpdate.checking') : t('appUpdate.check')}
      icon="search"
      help={elsewhere
        ? undefined
        : appUpdates.upToDate
          ? t('appUpdate.upToDate')
          : waitingForStable()
            ? t('appUpdate.nightlyWaiting')
            : undefined}
      disabled={appUpdates.checking || elsewhere}
      onclick={() => void appUpdates.check()}
    />
  {/if}
  {#if appUpdates.error}<p class="error" role="alert">
      {t(`errors.${appUpdates.error}`)}
    </p>{/if}
</Group>

<style>
  .error {
    margin: 0;
    padding: 0 var(--space-3) var(--space-2);
    color: var(--color-danger);
  }
</style>
