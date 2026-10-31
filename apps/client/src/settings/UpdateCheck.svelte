<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- About: how updates come, and a check right now. Only release builds
     update themselves, the others show nothing. -->
<script lang="ts">
  import { appUpdates, updatesHere } from '../lib/appUpdate.svelte'
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'

  const phone = !!window.AppInstaller
</script>

{#if updatesHere}
  <div class="updates">
    <label class="settings-check">
      <input type="checkbox" bind:checked={prefs.autoUpdates} />
      {t('appUpdate.auto')}
    </label>
    <p class="muted">{t('appUpdate.autoHelp')}</p>
    <p class="muted">
      {phone
        ? t('appUpdate.helpPhone')
        : __APP_PORTABLE__
          ? t('appUpdate.helpPortable')
          : t('appUpdate.help')}
    </p>
    {#if appUpdates.ready?.page}
      <p>{t('appUpdate.out', { version: appUpdates.ready.version })}</p>
      <button class="btn primary" onclick={() => void appUpdates.install()}
        >{t('appUpdate.download')}</button
      >
    {:else if appUpdates.ready}
      <p>{t('appUpdate.ready', { version: appUpdates.ready.version })}</p>
      <button class="btn primary" onclick={() => void appUpdates.install()}
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
  .error {
    color: var(--color-danger);
  }
</style>
