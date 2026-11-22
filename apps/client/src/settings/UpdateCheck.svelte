<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The updates group of About: how versions come, the nightly versions and a
     check right now. Only release builds update themselves, the others show
     nothing. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { backend } from '../lib/backend'
  import { ANDROID, LINUX } from '../lib/platform'
  import { channelSwitch } from '../lib/channelChange.svelte'
  import {
    appUpdates,
    channelChoice,
    downloadLine,
    notificationsHere,
    updatesHere,
    waitingForStable,
  } from '../lib/appUpdate.svelte'
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'

  const phone = ANDROID

  /** Installed as a Linux package, which the system updates. */
  let packaged = $state(false)
  onMount(() => {
    if (LINUX && updatesHere) void backend.appUpdatePageOnly().then((only) => (packaged = only))
  })

  /** The system refused notifications: says where to allow them. */
  let notifyDenied = $state(false)

  async function setNotifications(on: boolean) {
    notifyDenied = !(await appUpdates.setNotifications(on))
  }
</script>

{#if updatesHere}
  <!-- a package has no switch here, so the one line says where its updates come from -->
  <Group title={t('appUpdate.title')} note={packaged ? t('appUpdate.helpPackage') : undefined}>
    <!-- a package's updates are its package manager's: nothing to switch -->
    {#if !packaged}
      <!-- off, an urgent update is only a card like the others -->
      <Row
        label={t('appUpdate.securityChecks')}
        help={prefs.securityChecks ? undefined : t('appUpdate.securityChecksOff')}
        on={prefs.securityChecks}
        onclick={() => (prefs.securityChecks = !prefs.securityChecks)}
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
    {#if phone}
      <Row
        label={t('appUpdate.mobileData')}
        on={prefs.updatesOnMobileData}
        onclick={() => (prefs.updatesOnMobileData = !prefs.updatesOnMobileData)}
      />
    {/if}
    {#if channelChoice && !packaged}
      <Row
        label={t('appUpdate.nightlyVersions')}
        on={prefs.channel === 'nightly'}
        onclick={() =>
          void channelSwitch.choose(prefs.channel === 'nightly' ? 'stable' : 'nightly')}
      />
    {/if}
    {#if appUpdates.ready}
      <Row
        label={appUpdates.downloaded
          ? t('appUpdate.ready', { version: appUpdates.ready.version })
          : t('appUpdate.out', { version: appUpdates.ready.version })}
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
        help={appUpdates.upToDate
          ? t('appUpdate.upToDate')
          : waitingForStable()
            ? t('appUpdate.nightlyWaiting')
            : undefined}
        disabled={appUpdates.checking}
        onclick={() => void appUpdates.check()}
      />
    {/if}
    {#if appUpdates.error}<p class="error" role="alert">
        {t(`errors.${appUpdates.error}`)}
      </p>{/if}
  </Group>
{/if}

<style>
  .error {
    margin: 0;
    padding: 0 var(--space-3) var(--space-2);
    color: var(--color-danger);
  }
</style>
