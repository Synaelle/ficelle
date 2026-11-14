<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The dashboard's security checklist: what the server sees of itself
     (admins' second factors, sign-up, storage, backups, version, https, its
     address). Any line can be hidden, one that
     is fine once read, one that is not when the admin finds it fine as it
     is; it shows again once it says something else. -->
<script lang="ts">
  /** The server is for its admin alone: lines speak to that one person. */
  let { personal = false }: { personal?: boolean } = $props()
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { api, sectionHref } from './api.svelte'
  import { errorText, relative } from './format'

  type State = 'ok' | 'warn' | 'fail'
  interface Item {
    key: string
    state: State
    values?: Record<string, string>
    dismissed?: boolean
  }
  let items: Item[] = $state([])
  const shown = $derived(items.filter((i) => !i.dismissed))
  const hiddenCount = $derived(items.length - shown.length)
  /** What needs a look stays in view, what is fine folds into one line. */
  const needing = $derived(shown.filter((i) => i.state !== 'ok'))
  const fine = $derived(shown.filter((i) => i.state === 'ok'))

  /** The dashboard section where a line's setting lives, if any. */
  function place(item: Item): string | null {
    switch (item.key) {
      case 'backups':
        return 'backups'
      case 'storage':
        return 'storage'
      case 'secondFactors':
        return personal ? 'security' : 'people'
      default:
        return null
    }
  }
  let problem = $state('')
  let busy = $state(false)
  /** The lines that are fine, folded into their count until asked for. */
  let showFine = $state(false)

  async function load() {
    try {
      items = (await api<{ items: Item[] }>('GET', '/admin/checklist')).items
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

<section class="dash-group" aria-labelledby="checklist-title">
  <h2 id="checklist-title">
    {t('admin.checklist.title')}
    {#if fine.length}
      <button
        class="btn quiet dash-aside"
        aria-expanded={showFine}
        onclick={() => (showFine = !showFine)}
        >{t('admin.checklist.fineCount', { count: fine.length })}</button
      >
    {/if}
  </h2>
  {#if problem}<p class="dash-problem" role="alert">{problem}</p>{/if}
  {#snippet line(item: Item)}
    {@const where = place(item)}
    <li class={['dash-row', item.state]}>
      <span class="dash-lead"
        ><Icon name={item.state === 'ok' ? 'check' : 'warning'} size={16} /></span
      >
      <span class="dash-words">{text(item)}</span>
      <span class="dash-ctl">
        {#if where && item.state !== 'ok'}
          <a class="btn" href={sectionHref(where)}>{t('admin.checklist.goThere')}</a>
        {/if}
        <button
          class="btn quiet"
          disabled={busy}
          onclick={() => act(() => api('POST', '/admin/checklist/dismiss', { key: item.key }))}
          >{item.key === 'secrets'
            ? t('admin.checklist.secretsDone')
            : item.state === 'ok'
              ? t('admin.checklist.hide')
              : t('admin.checklist.fine')}</button
        >
      </span>
    </li>
  {/snippet}
  {#if needing.length || showFine}
    <ul class="dash-rows">
      {#each needing as item (item.key)}{@render line(item)}{/each}
      {#if showFine}
        {#each fine as item (item.key)}{@render line(item)}{/each}
      {/if}
    </ul>
  {/if}
  {#if hiddenCount > 0}
    <p class="dash-help">
      {t('admin.checklist.hidden', { count: hiddenCount })}
      <button
        class="link"
        disabled={busy}
        onclick={() => act(() => api('POST', '/admin/checklist/restore'))}
        >{t('admin.checklist.showHidden')}</button
      >
    </p>
  {/if}
</section>

<style>
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-accent-text);
    font: inherit;
    cursor: pointer;
    text-decoration: underline;
  }
  .link:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
