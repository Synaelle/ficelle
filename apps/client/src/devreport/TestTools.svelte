<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The test tools section of dev and preview builds: where issue reports go
     and whether they start with a screenshot. -->
<script lang="ts">
  import { backend } from '../lib/backend'
  import { t } from '../lib/i18n.svelte'
  import { pickToOpen } from '../lib/pick'
  import { prefs } from '../lib/prefs.svelte'
  import { desktopFolder, phoneFolder, phoneReports, pickPhoneFolder } from './report'

  const phone = phoneReports() !== undefined
  let folder = $state(phone ? phoneFolder() : '')
  let problem = $state('')

  $effect(() => {
    // the web preview has no app side to ask
    if (phone || !backend.native) return
    void prefs.reportFolder
    desktopFolder()
      .then((path) => {
        folder = path
        problem = ''
      })
      .catch((err: unknown) => {
        folder = ''
        problem = String((err as { message?: string }).message ?? err)
      })
  })

  async function choose() {
    if (phone) {
      folder = await pickPhoneFolder()
      return
    }
    const picked = await pickToOpen({ directory: true, title: t('testTools.choose') })
    if (picked) prefs.reportFolder = picked
  }
</script>

<p class="settings-help">{t('testTools.help')}</p>
<div class="settings-group">
  <h3>{t('testTools.folder')}</h3>
  {#if folder}
    <code data-report-folder>{folder}</code>
  {:else if problem}
    <p class="problem small">{problem}</p>
  {:else if phone}
    <p class="problem small">{t('testTools.noFolder')}</p>
  {/if}
  <p class="settings-help">{phone ? t('testTools.folderHelpPhone') : t('testTools.folderHelp')}</p>
  <div class="settings-actions">
    <button class="btn" onclick={choose}>{t('testTools.choose')}</button>
    {#if !phone && prefs.reportFolder}
      <button class="btn" onclick={() => (prefs.reportFolder = null)}
        >{t('testTools.default')}</button
      >
    {/if}
  </div>
</div>
<div class="settings-group">
  <label class="settings-check">
    <input type="checkbox" bind:checked={prefs.reportScreenshot} />
    {t('testTools.screenshot')}
  </label>
  <p class="settings-help">{t('testTools.screenshotHelp')}</p>
</div>

<style>
  code {
    overflow-wrap: anywhere;
    font-family: var(--font-mono);
    font-size: 13px;
  }
</style>
