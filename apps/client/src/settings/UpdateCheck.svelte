<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Updates, the same on desktop and phone: how versions come, the nightly
     versions and a check right now. Only listed where the app has updates.
     When a store or a package manager brings them, one line says so and the
     rows stay, greyed. A phone has no update card, only a mark that opens
     this page: Later and the list of changes are here instead. -->
<script lang="ts">
  import ErrorText from '../lib/ErrorText.svelte'
  import { ANDROID, PHONE } from '../lib/platform'
  import { channelSwitch } from '../lib/channelChange.svelte'
  import {
    appUpdates,
    changelogLabelled,
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

  /** The release's list of changes in English, or its English line. */
  const details = $derived(
    PHONE ? appUpdates.ready?.changelog || appUpdates.ready?.notes || '' : '',
  )

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
      icon="bell"
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
      {#if PHONE && appUpdates.offered}
        <button
          class="btn"
          aria-haspopup={appUpdates.ready.critical ? undefined : 'menu'}
          onclick={(e) => appUpdates.later(e.currentTarget)}>{t('appUpdate.later')}</button
        >
      {/if}
    </Row>
    {#if details}
      <!-- focusable, so the keyboard can scroll long notes -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="changelog" tabindex="0" aria-label={t('appUpdate.changes')}>
        <span class="caption">{t('appUpdate.changes')}</span>
        {#if changelogLabelled()}<span class="lang">{t('appUpdate.message.inEnglish')}</span>{/if}
        <p lang="en">{details}</p>
      </div>
    {/if}
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
      <ErrorText error={appUpdates.error} />
    </p>{/if}
</Group>

<style>
  .changelog {
    max-height: 14em;
    overflow-y: auto;
    padding: var(--space-2) var(--space-3) var(--space-3);
    color: var(--color-text-muted);
    line-height: 1.45;
    overflow-wrap: anywhere;
  }
  .changelog p {
    margin: 0;
    white-space: pre-line;
  }
  .caption {
    display: block;
    margin-bottom: var(--space-1);
    color: var(--color-text);
    font-weight: 600;
  }
  .lang {
    display: block;
    margin-bottom: var(--space-1);
    font-size: 12px;
    font-weight: 600;
  }
  .error {
    margin: 0;
    padding: 0 var(--space-3) var(--space-2);
    color: var(--color-danger);
  }
</style>
