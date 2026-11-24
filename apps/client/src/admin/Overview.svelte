<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The server at a glance: a strip of its state (devices, storage, the
     last backup, new feedback), what needs a look, and its on or off
     settings. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api, sectionHref, type Overview, type StorageOverview } from './api.svelte'
  import Checklist from './Checklist.svelte'
  import { bytes, errorText, relative } from './format'

  interface Updates {
    enabled: boolean
    available: boolean
    latest?: { version: string; date: string; notes: string; url: string }
    checkedAt?: number
    error?: string
  }
  interface Backups {
    config: unknown
    status: { lastSuccessAt: number | null; lastError: string | null }
  }

  let overview: Overview | null = $state(null)
  let storage: StorageOverview | null = $state(null)
  let backups = $state<Backups | null>(null)
  let newReports: number | null = $state(null)
  let updates: Updates | null = $state(null)
  let problem = $state('')
  let busy = $state(false)

  async function load() {
    try {
      ;[overview, storage, backups, updates] = await Promise.all([
        api<Overview>('GET', '/admin/overview'),
        api<StorageOverview>('GET', '/admin/storage'),
        api<Backups>('GET', '/admin/backups'),
        api<Updates>('GET', '/admin/updates'),
      ])
      newReports = (
        await api<{ counts: { new: number } }>('GET', '/admin/feedback/reports?status=new&limit=1')
      ).counts.new
    } catch (err) {
      problem = errorText(err)
    }
  }

  async function act(action: () => Promise<unknown>) {
    busy = true
    problem = ''
    try {
      await action()
      await load()
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  async function setSecondStep(required: boolean, box: HTMLInputElement) {
    // a password alone for every admin: said before it happens
    if (
      !required &&
      !(await overlay.ask(t('admin.overview.secondStepOffConfirmPersonal'), {
        confirmLabel: t('admin.overview.secondStepOffYes'),
        danger: true,
      }))
    ) {
      box.checked = true
      return
    }
    await act(() => api('PUT', '/admin/second-step', { required }))
  }

  const lastBackup = $derived(
    !backups
      ? null
      : !backups.config
        ? { v: t('admin.overview.backupNone'), tone: 'dash-warn' }
        : backups.status.lastError
          ? { v: t('admin.overview.backupFailed'), tone: 'dash-bad' }
          : backups.status.lastSuccessAt
            ? { v: relative(backups.status.lastSuccessAt), tone: 'dash-ok' }
            : { v: t('admin.overview.backupNever'), tone: 'dash-warn' },
  )

  void load()
</script>

{#if problem}<p class="dash-problem" role="alert">{problem}</p>{/if}
{#if !overview && !problem}<p class="muted" aria-busy="true">{t('admin.loading')}</p>{/if}

{#if overview}
  <div class="dash-strip">
    <a href={sectionHref('security')}>
      <span class="k">{t('admin.overview.devices')}</span>
      <span class="v">{overview.devices}</span>
    </a>
    {#if storage}
      <a href={sectionHref('advanced')}>
        <span class="k">{t('admin.storage.title')}</span>
        <span class="v">{bytes(storage.storedBytes)}</span>
        <span class="s">{t('admin.overview.files', { count: storage.files })}</span>
      </a>
    {/if}
    {#if lastBackup}
      <a href={sectionHref('backups')}>
        <span class="k">{t('admin.overview.lastBackup')}</span>
        <span class={['v', lastBackup.tone]}>{lastBackup.v}</span>
      </a>
    {/if}
    {#if newReports !== null}
      <a href={sectionHref('feedback')}>
        <span class="k">{t('admin.overview.newFeedback')}</span>
        <span class="v">{newReports}</span>
      </a>
    {/if}
  </div>
{/if}

<Checklist />

{#if overview}
  <section class="dash-group" aria-labelledby="server-settings">
    <h2 id="server-settings">{t('admin.groupServer')}</h2>
    <ul class="dash-rows">
      {#if updates}
        <li class="dash-row">
          <span class="dash-words">
            <span class="dash-label">{t('admin.checklist.updatesTitle')}</span>
            <span class="dash-help">
              {#if !updates.available}
                {t('admin.checklist.updatesUnavailable')}
              {:else if updates.enabled}
                {updates.checkedAt
                  ? t('admin.checklist.checked', { when: relative(updates.checkedAt) })
                  : t('admin.checklist.notChecked')}
                {#if updates.latest}
                  {t('admin.checklist.latest', {
                    version: updates.latest.version,
                    date: updates.latest.date,
                  })}
                  {#if updates.latest.url}
                    <a href={updates.latest.url} target="_blank" rel="noreferrer"
                      >{t('admin.checklist.readMore')}</a
                    >
                  {/if}
                {/if}
              {:else}
                {t('admin.checklist.updatesSwitch')}
              {/if}
            </span>
            {#if updates.enabled && updates.error}
              <span class="dash-help dash-bad"
                >{t('admin.checklist.checkFailed', { why: updates.error })}</span
              >
            {/if}
          </span>
          {#if updates.available}
            <span class="dash-ctl">
              {#if updates.enabled}
                <button
                  class="btn"
                  disabled={busy}
                  onclick={() => act(() => api('POST', '/admin/updates/check'))}
                  >{t('admin.checklist.checkNow')}</button
                >
              {/if}
              <input
                class="dash-switch"
                type="checkbox"
                role="switch"
                aria-label={t('admin.checklist.updatesSwitch')}
                checked={updates.enabled}
                disabled={busy}
                onchange={(e) => {
                  const enabled = e.currentTarget.checked
                  void act(() => api('PUT', '/admin/updates', { enabled }))
                }}
              />
            </span>
          {/if}
        </li>
      {/if}
      <li class="dash-row">
        <span class="dash-words">
          <span class="dash-label">{t('admin.overview.secondStepPersonal')}</span>
          <span class="dash-help">{t('admin.overview.secondStepHelpPersonal')}</span>
          {#if !overview.adminsNeedSecondStep}
            <span class="dash-help dash-bad">{t('admin.overview.secondStepOffRisksPersonal')}</span>
          {/if}
        </span>
        <span class="dash-ctl">
          <input
            class="dash-switch"
            type="checkbox"
            role="switch"
            aria-label={t('admin.overview.secondStepOnPersonal')}
            checked={overview.adminsNeedSecondStep}
            disabled={busy}
            onchange={(e) => setSecondStep(e.currentTarget.checked, e.currentTarget)}
          />
        </span>
      </li>
      <li class="dash-row">
        <span class="dash-words">
          <span class="dash-label">{t('admin.overview.coediting')}</span>
          <span class="dash-help">{t('admin.overview.coeditingHelpPersonal')}</span>
        </span>
        <span class="dash-ctl">
          <input
            class="dash-switch"
            type="checkbox"
            role="switch"
            aria-label={t('admin.overview.coeditingOn')}
            checked={overview.coediting}
            disabled={busy}
            onchange={(e) => {
              const coediting = e.currentTarget.checked
              void act(() => api('PUT', '/admin/coediting', { coediting }))
            }}
          />
        </span>
      </li>
    </ul>
  </section>

  <p class="dash-foot">
    {t('admin.overview.since', {
      date: overview.createdAt.slice(0, 10),
      version: overview.version,
    })}
  </p>
{/if}
