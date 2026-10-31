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

<div class="bar">
  <label>
    {t('admin.log.filter')}
    <select class="field" bind:value={event} onchange={() => load()}>
      <option value="">{t('admin.log.all')}</option>
      {#each EVENTS as name (name)}<option value={name}>{label(name)}</option>{/each}
    </select>
  </label>
</div>
{#if problem}<p class="problem" role="alert">{problem}</p>{/if}

<table>
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
        <td class="nowrap">{dateTime(entry.at)}</td>
        <td>{label(entry.event)}</td>
        <td>{entry.username ?? (entry.userId ? t('admin.log.deleted') : '–')}</td>
        <td class="hide-narrow mono">{entry.ip ?? ''}</td>
        <td class="hide-narrow">{entry.detail ?? ''}</td>
      </tr>
    {:else}
      <tr><td colspan="5" class="muted">{t('admin.log.none')}</td></tr>
    {/each}
  </tbody>
</table>
{#if next}
  <button class="btn more" onclick={() => load(true)}>{t('admin.log.older')}</button>
{/if}

<style>
  .bar {
    margin-bottom: var(--space-3);
  }
  label {
    display: inline-grid;
    gap: var(--space-1);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    text-align: left;
    color: var(--color-text-muted);
    font-size: 13px;
  }
  th,
  td {
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--color-border);
    vertical-align: top;
  }
  .alarm td {
    color: var(--color-danger);
  }
  .nowrap {
    white-space: nowrap;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .more {
    margin-top: var(--space-3);
  }
  .problem {
    color: var(--color-danger);
  }
  @media (max-width: 760px) {
    .hide-narrow {
      display: none;
    }
  }
</style>
