<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The updates group of About: how versions come, the beta versions and a
     check right now. Only release builds update themselves, the others show
     nothing. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { backend } from '../lib/backend'
  import { ANDROID, LINUX } from '../lib/platform'
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

  const phone = ANDROID

  /** Turning the security checks off waits a few seconds on a warning. */
  async function securityOff() {
    const yes = await overlay.ask(t('appUpdate.securityOffWarning'), {
      confirmLabel: t('appUpdate.securityOff'),
      danger: true,
      wait: 5,
    })
    if (yes) prefs.securityChecks = false
  }

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
          : packaged
            ? t('appUpdate.helpPackage')
            : t('appUpdate.help'),
  )
</script>

{#if updatesHere}
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
    {#if channelChoice}
      <Row
        label={t('appUpdate.betaVersions')}
        help={t('appUpdate.betaHelp')}
        on={prefs.channel === 'beta'}
        onclick={() => void channelSwitch.choose(prefs.channel === 'beta' ? 'stable' : 'beta')}
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
