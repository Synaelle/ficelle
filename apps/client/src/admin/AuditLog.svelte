<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The security log, newest first, filtered by event. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { api, type AuditEntry } from './api.svelte'
  import { dateTime, errorText } from './format'

  const EVENTS = [
    'sign_in',
    'sign_in_failed',
    'locked_out',
    'token_reuse',
    'signed_out',
    'device_revoked',
    'password_changed',
    'password_reset',
    'registered',
    'user_created',
    'user_updated',
    'user_deleted',
    'invite_created',
    'invite_revoked',
    'registration_changed',
    'group_created',
    'group_renamed',
    'group_deleted',
    'group_member_added',
    'group_member_removed',
    'setup_done',
    'setup_failed',
    'feedback_settings',
    'feedback_reader_added',
    'feedback_reader_removed',
    'feedback_token',
    'feedback_blocked',
    'feedback_unblocked',
    'feedback_deleted',
    'web_version_changed',
    'signin_limits_changed',
  ]

  let event = $state('')
  let entries: AuditEntry[] = $state([])
  let next: number | null = $state(null)
  let problem = $state('')

  /** A known event in words, an unknown one as it is. */
  function label(name: string) {
    const key = `admin.log.events.${name}`
    const text = t(key)
    return text === key ? name : text
  }

  async function load(more = false) {
    problem = ''
    const params = new URLSearchParams({
      limit: '100',
      ...(event ? { event } : {}),
      ...(more && next ? { before: String(next) } : {}),
    })
    try {
      const page = await api<{ items: AuditEntry[]; nextBefore: number | null }>(
        'GET',
        `/admin/audit?${params}`,
      )
      entries = more ? [...entries, ...page.items] : page.items
      next = page.nextBefore
    } catch (err) {
      problem = errorText(err)
    }
  }

  void load()
</script>

<section class="dash-group" aria-labelledby="log-title">
  <h2 id="log-title">
    {t('admin.log.title')}
    <select
      class="field dash-select dash-aside"
      aria-label={t('admin.log.filter')}
      bind:value={event}
      onchange={() => load()}
    >
      <option value="">{t('admin.log.all')}</option>
      {#each EVENTS as name (name)}<option value={name}>{label(name)}</option>{/each}
    </select>
  </h2>
  {#if problem}<p class="dash-problem" role="alert">{problem}</p>{/if}
  <div class="dash-table-box">
    <table class="dash-table">
      <thead>
        <tr>
          <th>{t('admin.log.when')}</th>
          <th>{t('admin.log.what')}</th>
          <th>{t('admin.log.who')}</th>
          <th class="hide-narrow">{t('admin.log.address')}</th>
          <th class="hide-narrow">{t('admin.log.detail')}</th>
        </tr>
      </thead>
      <tbody>
        {#each entries as entry (entry.id)}
          <tr class:alarm={['token_reuse', 'locked_out', 'setup_failed'].includes(entry.event)}>
            <td>{dateTime(entry.at)}</td>
            <td>{label(entry.event)}</td>
            <td>{entry.username ?? (entry.userId ? t('admin.log.deleted') : '–')}</td>
            <td class="hide-narrow mono">{entry.ip ?? ''}</td>
            <td class="hide-narrow detail">{entry.detail ?? ''}</td>
          </tr>
        {:else}
          <tr><td colspan="5" class="muted">{t('admin.log.none')}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
  {#if next}
    <p><button class="btn" onclick={() => load(true)}>{t('admin.log.older')}</button></p>
  {/if}
</section>

<style>
  .alarm td {
    color: var(--color-danger);
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .detail {
    white-space: normal;
  }
  @media (max-width: 760px) {
    .hide-narrow {
      display: none;
    }
  }
</style>
