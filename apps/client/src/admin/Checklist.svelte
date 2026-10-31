<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The dashboard's security checklist: what the server sees of itself
     (admins' second factors, sign-up, storage, backups, version, https, its address)
     and the update check with its switch. A line that is not fine can be
     hidden when the admin finds it fine as it is; it shows again once it
     says something else. -->
<script lang="ts">
  /** The server is for its admin alone: lines speak to that one person. */
  let { personal = false }: { personal?: boolean } = $props()
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { api } from './api.svelte'
  import { errorText, relative } from './format'

  type State = 'ok' | 'warn' | 'fail'
  interface Item {
    key: string
    state: State
    values?: Record<string, string>
    dismissed?: boolean
  }
  interface Release {
    version: string
    date: string
    security: boolean
    notes: string
    url: string
  }
  interface Updates {
    enabled: boolean
    available: boolean
    current: string
    latest?: Release
    newer: boolean
    security: boolean
    checkedAt?: number
    error?: string
  }

  let items: Item[] = $state([])
  const shown = $derived(items.filter((i) => !i.dismissed))
  const hiddenCount = $derived(items.length - shown.length)
  let updates: Updates | null = $state(null)
  let problem = $state('')
  let busy = $state(false)

  async function load() {
    try {
      const [list, view] = await Promise.all([
        api<{ items: Item[] }>('GET', '/admin/checklist'),
        api<Updates>('GET', '/admin/updates'),
      ])
      items = list.items
      updates = view
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

  const when = (at = '') => (at ? relative(Number(at)) : '')

  /** Each line in words. Written out case by case so every string stays
   * visible to the translation check. */
  function text(item: Item): string {
    const v = item.values ?? {}
    switch (item.key) {
      case 'secondFactors':
        if (personal)
          return item.state === 'ok'
            ? t('admin.checklist.factorsOkPersonal')
            : t('admin.checklist.factorsMissingPersonal')
        return item.state === 'ok'
          ? t('admin.checklist.factorsOk')
          : t('admin.checklist.factorsMissing', { names: v.names ?? '' })
      case 'registration':
        return v.mode === 'open'
          ? t('admin.checklist.registrationOpen')
          : v.mode === 'invite'
            ? t('admin.checklist.registrationInvite')
            : personal
              ? t('admin.checklist.registrationClosedPersonal')
              : t('admin.checklist.registrationClosed')
      case 'storage':
        if (personal)
          return v.state === 'unlimited'
            ? t('admin.checklist.storageUnlimitedPersonal')
            : t('admin.checklist.storageLimitPersonal', { mb: v.mb ?? '' })
        return v.state === 'unlimited'
          ? t('admin.checklist.storageUnlimited')
          : t('admin.checklist.storageLimit', { mb: v.mb ?? '' })
      case 'backups':
        switch (v.state) {
          case 'none':
            return t('admin.checklist.backupsNone')
          case 'failing':
            return t('admin.checklist.backupsFailing', { error: v.error ?? '' })
          case 'never':
            return t('admin.checklist.backupsNever')
          case 'late':
            return t('admin.checklist.backupsLate', { when: when(v.at) })
          case 'manual':
            return t('admin.checklist.backupsManual', { when: when(v.at) })
          default:
            return t('admin.checklist.backupsFine', { when: when(v.at) })
        }
      case 'updates':
        switch (v.state) {
          case 'off':
            return t('admin.checklist.updatesOff')
          case 'security':
            return t('admin.checklist.updatesSecurity', {
              latest: v.latest ?? '',
              current: v.current ?? '',
            })
          case 'newer':
            return t('admin.checklist.updatesNewer', {
              latest: v.latest ?? '',
              current: v.current ?? '',
            })
          case 'waiting':
            return t('admin.checklist.updatesWaiting', { current: v.current ?? '' })
          default:
            return t('admin.checklist.updatesCurrent', { current: v.current ?? '' })
        }
      case 'tls':
        return item.state === 'ok'
          ? t('admin.checklist.tlsOk')
          : v.state === 'local'
            ? personal
              ? t('admin.checklist.tlsLocalPersonal')
              : t('admin.checklist.tlsLocal')
            : t('admin.checklist.tlsPlain')
      case 'address':
        return v.state === 'unset'
          ? t('admin.checklist.addressUnset')
          : v.state === 'http'
            ? t('admin.checklist.addressHttp', { url: v.url ?? '' })
            : t('admin.checklist.addressOk', { url: v.url ?? '' })
      default:
        return v.apart === 'true'
          ? t('admin.checklist.secretsApart')
          : personal
            ? t('admin.checklist.secretsPersonal')
            : t('admin.checklist.secrets')
    }
  }

  void load()
</script>

<section class="block" aria-labelledby="checklist-title">
  <h2 id="checklist-title">
    <Icon name="shield" size={18} />{t('admin.checklist.title')}
  </h2>
  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
  <ul class="items">
    {#each shown as item (item.key)}
      <li class={item.state}>
        <Icon name={item.state === 'ok' ? 'check' : 'info'} size={16} />
        <span>{text(item)}</span>
        {#if item.state !== 'ok'}
          <button
            class="btn"
            disabled={busy}
            onclick={() => act(() => api('POST', '/admin/checklist/dismiss', { key: item.key }))}
            >{item.key === 'secrets'
              ? t('admin.checklist.secretsDone')
              : t('admin.checklist.fine')}</button
          >
        {/if}
      </li>
    {/each}
  </ul>
  {#if hiddenCount > 0}
    <p class="muted">
      {t('admin.checklist.hidden', { count: hiddenCount })}
      <button
        class="link"
        disabled={busy}
        onclick={() => act(() => api('POST', '/admin/checklist/restore'))}
        >{t('admin.checklist.showHidden')}</button
      >
    </p>
  {/if}

  {#if updates}
    <h3>{t('admin.checklist.updatesTitle')}</h3>
    {#if !updates.available}
      <p class="muted">{t('admin.checklist.updatesUnavailable')}</p>
    {:else}
      <label class="switch">
        <input
          type="checkbox"
          checked={updates.enabled}
          disabled={busy}
          onchange={(e) => {
            const enabled = e.currentTarget.checked
            void act(() => api('PUT', '/admin/updates', { enabled }))
          }}
        />
        {t('admin.checklist.updatesSwitch')}
      </label>
      {#if updates.enabled}
        <div class="row">
          <button
            class="btn"
            disabled={busy}
            onclick={() => act(() => api('POST', '/admin/updates/check'))}
            >{t('admin.checklist.checkNow')}</button
          >
          <span class="muted">
            {updates.checkedAt
              ? t('admin.checklist.checked', { when: relative(updates.checkedAt) })
              : t('admin.checklist.notChecked')}
          </span>
        </div>
        {#if updates.error}
          <p class="problem">{t('admin.checklist.checkFailed', { why: updates.error })}</p>
        {/if}
        {#if updates.latest}
          <p>
            {t('admin.checklist.latest', {
              version: updates.latest.version,
              date: updates.latest.date,
            })}
            {#if updates.latest.notes}<span class="muted">{updates.latest.notes}</span>{/if}
            {#if updates.latest.url}
              <a href={updates.latest.url} target="_blank" rel="noreferrer"
                >{t('admin.checklist.readMore')}</a
              >
            {/if}
          </p>
        {/if}
      {/if}
    {/if}
  {/if}
</section>

<style>
  .block {
    display: grid;
    gap: var(--space-3);
    margin-bottom: var(--space-5);
  }
  h2 {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    margin: 0;
    font-size: 16px;
  }
  h3 {
    margin: var(--space-2) 0 0;
    font-size: 14px;
  }
  p {
    margin: 0;
  }
  .items {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .items li {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    align-items: center;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-left-width: 3px;
    border-radius: var(--radius-popover);
  }
  .items li span {
    flex: 1;
    min-width: 200px;
  }
  .items li.ok {
    border-left-color: var(--color-success);
  }
  .items li.warn {
    border-left-color: var(--color-warning);
  }
  .items li.fail {
    border-left-color: var(--color-danger);
  }
  .switch {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    align-items: center;
  }
  .problem {
    color: var(--color-danger);
  }
</style>
