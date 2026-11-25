<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The security log, newest first, filtered by event. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { api, type AuditEntry } from './api.svelte'
  import { dateTime, errorText } from './format'

  /** Every event the server writes. Older entries may hold kinds it no
   * longer writes. */
  const EVENTS = [
    'sign_in',
    'sign_in_failed',
    'locked_out',
    'token_reuse',
    'signed_out',
    'app_connect',
    'password_changed',
    'password_change_failed',
    'password_recovered',
    'password_strengthened',
    'confirm_failed',
    'second_factor_failed',
    'totp_enabled',
    'totp_disabled',
    'recovery_codes_created',
    'passkey_added',
    'passkey_removed',
    'passkey_failed',
    'data_exported',
    'setup_started',
    'setup_done',
    'setup_failed',
    'admin_created',
    'admin_reset',
    'admin_second_step_changed',
    'signin_limits_changed',
    'coediting_changed',
    'policy_changed',
    'updates_check',
    'backup_set_up',
    'backup_stopped',
    'backup_locked',
    'backup_done',
    'backup_failed',
    'feedback_settings',
    'feedback_caps',
    'feedback_token',
    'feedback_sent',
    'feedback_blocked',
    'feedback_unblocked',
    'feedback_deleted',
  ]

  let event = $state('')
  let entries: AuditEntry[] = $state([])
  let next: number | null = $state(null)
  let problem = $state('')

  /** An event in words. A kind the server no longer writes says only that
   * it comes from an earlier version. */
  function label(name: string) {
    const key = `admin.log.events.${name}`
    const text = t(key)
    return text === key ? t('admin.log.earlier') : text
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
    font-family: var(--font-ui);
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
