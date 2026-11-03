<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The test tools section of dev and preview builds: where issue reports go
     and whether they start with a screenshot. -->
<script lang="ts">
  import Group from '../settings/ui/Group.svelte'
  import Row from '../settings/ui/Row.svelte'
  import Switch from '../settings/ui/Switch.svelte'
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
<Group
  note={!phone
    ? t('testTools.folderHelp')
    : folder
      ? t('testTools.folderHelpChosen')
      : t('testTools.folderHelpPhone')}
>
  <Row label={t('testTools.folder')}>
    {#if !phone && prefs.reportFolder}
      <button class="btn" onclick={() => (prefs.reportFolder = null)}
        >{t('testTools.default')}</button
      >
    {/if}
    <button class="btn" onclick={choose}>{t('testTools.choose')}</button>
  </Row>
  {#if folder}
    <div class="settings-row where"><code data-report-folder>{folder}</code></div>
  {:else if problem}
    <div class="settings-row where"><p class="problem small">{problem}</p></div>
  {:else if phone}
    <div class="settings-row where"><p class="problem small">{t('testTools.noFolder')}</p></div>
  {/if}
</Group>
<Group>
  <Row label={t('testTools.screenshot')} help={t('testTools.screenshotHelp')}>
    <Switch label={t('testTools.screenshot')} bind:checked={prefs.reportScreenshot} />
  </Row>
  <Row label={t('testTools.pretendBeta')} help={t('testTools.pretendBetaHelp')}>
    <Switch
      label={t('testTools.pretendBeta')}
      checked={prefs.pretendBeta}
      onchange={(on) => {
        prefs.pretendBeta = on
        // read once at start (lib/beta.svelte.ts): saved first, then reloaded
        try {
          localStorage.setItem('prefs', JSON.stringify(prefs))
        } catch {
          // not saved: the reload keeps the build as it was
        }
        location.reload()
      }}
    />
  </Row>
</Group>

<style>
  .where {
    position: relative;
    padding: var(--space-2) var(--space-3);
  }
  .where p {
    margin: 0;
  }
  code {
    overflow-wrap: anywhere;
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    font-size: 12px;
  }
</style>
