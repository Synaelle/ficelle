<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The server at a glance: a strip of its state (devices, storage, the
     last backup, the web version, new feedback), what needs a look, and its
     on or off settings. It is for one person; one with several accounts is
     pointed to going to one. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api, type Overview, type StorageOverview, type WebView } from './api.svelte'
  import Checklist from './Checklist.svelte'
  import { bytes, errorText, relative } from './format'

  let { several = false, ongo }: { several?: boolean; ongo?: () => void } = $props()

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
  let web: WebView | null = $state(null)
  let newReports: number | null = $state(null)
  let updates: Updates | null = $state(null)
  let problem = $state('')
  let busy = $state(false)

  async function load() {
    try {
      ;[overview, storage, backups, web, updates] = await Promise.all([
        api<Overview>('GET', '/admin/overview'),
        api<StorageOverview>('GET', '/admin/storage'),
        api<Backups>('GET', '/admin/backups'),
        api<WebView>('GET', '/admin/web'),
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

  /** Texts for a server of one person, who is its only admin. */
  const ownWords = 'Personal'

  async function setSecondStep(required: boolean, box: HTMLInputElement) {
    // a password alone for every admin: said before it happens
    if (
      !required &&
      !(await overlay.ask(t(`admin.overview.secondStepOffConfirm${ownWords}`), {
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
        ? { v: t('admin.overview.backupNone'), tone: 'ad-warn' }
        : backups.status.lastError
          ? { v: t('admin.overview.backupFailed'), tone: 'ad-bad' }
          : backups.status.lastSuccessAt
            ? { v: relative(backups.status.lastSuccessAt), tone: 'ad-ok' }
            : { v: t('admin.overview.backupNever'), tone: 'ad-warn' },
  )

  void load()
</script>

{#if problem}<p class="ad-problem" role="alert">{problem}</p>{/if}
{#if !overview && !problem}<p class="muted" aria-busy="true">{t('admin.loading')}</p>{/if}
{#if several}
  <p class="ad-problem" role="alert">
    {t('admin.onePerson.pointer')}
    <button class="btn" onclick={ongo}>{t('admin.onePerson.title')}</button>
  </p>
{/if}

{#if overview}
  <div class="ad-strip">
    <a href="#security">
      <span class="k">{t('admin.overview.devices')}</span>
      <span class="v">{overview.devices}</span>
    </a>
    {#if storage}
      <a href="#storage">
        <span class="k">{t('admin.storage.title')}</span>
        <span class="v">{bytes(storage.storedBytes)}</span>
        <span class="s">{t('admin.overview.files', { count: storage.files })}</span>
      </a>
    {/if}
    {#if lastBackup}
      <a href="#backups">
        <span class="k">{t('admin.overview.lastBackup')}</span>
        <span class={['v', lastBackup.tone]}>{lastBackup.v}</span>
      </a>
    {/if}
    {#if web}
      <a href="#web">
        <span class="k">{t('admin.web.title')}</span>
        <span class="v">{web.access === 'off' ? t('admin.web.tagOff') : t('admin.web.tagOn')}</span>
        <span class="s">{t(`admin.web.short_${web.access}`)}</span>
      </a>
    {/if}
    {#if newReports !== null}
      <a href="#feedback">
        <span class="k">{t('admin.overview.newFeedback')}</span>
        <span class="v">{newReports}</span>
      </a>
    {/if}
  </div>
{/if}

<Checklist personal />

{#if overview}
  <section class="ad-group" aria-labelledby="server-settings">
    <h2 id="server-settings">{t('admin.groupServer')}</h2>
    <ul class="ad-rows">
      {#if updates}
        <li class="ad-row">
          <span class="ad-words">
            <span class="ad-label">{t('admin.checklist.updatesTitle')}</span>
            <span class="ad-help">
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
              <span class="ad-help ad-bad"
                >{t('admin.checklist.checkFailed', { why: updates.error })}</span
              >
            {/if}
          </span>
          {#if updates.available}
            <span class="ad-ctl">
              {#if updates.enabled}
                <button
                  class="btn"
                  disabled={busy}
                  onclick={() => act(() => api('POST', '/admin/updates/check'))}
                  >{t('admin.checklist.checkNow')}</button
                >
              {/if}
              <input
                class="ad-switch"
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
      <li class="ad-row">
        <span class="ad-words">
          <span class="ad-label">{t(`admin.overview.secondStep${ownWords}`)}</span>
          <span class="ad-help">{t(`admin.overview.secondStepHelp${ownWords}`)}</span>
          {#if !overview.adminsNeedSecondStep}
            <span class="ad-help ad-bad">{t(`admin.overview.secondStepOffRisks${ownWords}`)}</span>
          {/if}
        </span>
        <span class="ad-ctl">
          <input
            class="ad-switch"
            type="checkbox"
            role="switch"
            aria-label={t(`admin.overview.secondStepOn${ownWords}`)}
            checked={overview.adminsNeedSecondStep}
            disabled={busy}
            onchange={(e) => setSecondStep(e.currentTarget.checked, e.currentTarget)}
          />
        </span>
      </li>
      <li class="ad-row">
        <span class="ad-words">
          <span class="ad-label">{t('admin.overview.coediting')}</span>
          <span class="ad-help">{t(`admin.overview.coeditingHelp${ownWords}`)}</span>
        </span>
        <span class="ad-ctl">
          <input
            class="ad-switch"
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

  <p class="ad-foot">
    {t('admin.overview.since', {
      date: overview.createdAt.slice(0, 10),
      version: overview.version,
    })}
  </p>
{/if}
