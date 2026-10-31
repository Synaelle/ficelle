<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Invite links: make one, copy it once, revoke the ones not used yet. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { api, type Invite } from './api.svelte'
  import { dateTime, errorText } from './format'

  let invites: Invite[] = $state([])
  /** How long the next invite lasts: days, or 0 for one hour. */
  let days = $state(7)
  let fresh: Invite | null = $state(null)
  let copied = $state(false)
  let problem = $state('')

  async function load() {
    try {
      invites = await api<Invite[]>('GET', '/invites')
    } catch (err) {
      problem = errorText(err)
    }
  }

  async function create() {
    problem = ''
    copied = false
    try {
      fresh = await api<Invite>('POST', '/invites', days === 0 ? { hours: 1 } : { days })
      await load()
    } catch (err) {
      problem = errorText(err)
    }
  }

  async function copy() {
    if (!fresh) return
    try {
      await navigator.clipboard.writeText(fresh.link ?? fresh.token ?? '')
      copied = true
    } catch {
      problem = t('admin.invites.copyFailed')
    }
  }

  async function revoke(id: string) {
    problem = ''
    try {
      await api('DELETE', `/invites/${id}`)
      if (fresh?.id === id) fresh = null
      await load()
    } catch (err) {
      problem = errorText(err)
    }
  }

  void load()
</script>

<p class="muted">{t('admin.invites.help')}</p>
<div class="bar">
  <label>
    {t('admin.invites.days')}
    <select class="field" bind:value={days}>
      <option value={0}>{t('admin.invites.oneHour')}</option>
      {#each [1, 3, 7, 14, 30] as d (d)}<option value={d}
          >{t('admin.invites.dayCount', { count: d })}</option
        >{/each}
    </select>
  </label>
  <button class="btn primary" onclick={create}
    ><Icon name="invite" size={16} />{t('admin.invites.create')}</button
  >
</div>

{#if problem}<p class="problem" role="alert">{problem}</p>{/if}

{#if fresh}
  <div class="fresh" role="status">
    <p>{fresh.link ? t('admin.invites.linkReady') : t('admin.invites.codeReady')}</p>
    <code>{fresh.link ?? fresh.token}</code>
    <button class="btn" onclick={copy}>
      <Icon name={copied ? 'check' : 'copy'} size={16} />{copied
        ? t('admin.invites.copied')
        : t('admin.invites.copy')}
    </button>
    <p class="muted">{t('admin.invites.once')}</p>
  </div>
{/if}

<table>
  <thead>
    <tr>
      <th>{t('admin.invites.state')}</th>
      <th>{t('admin.invites.by')}</th>
      <th>{t('admin.invites.expires')}</th>
      <th><span class="visually-hidden">{t('admin.invites.revoke')}</span></th>
    </tr>
  </thead>
  <tbody>
    {#each invites as invite (invite.id)}
      <tr>
        <td>{t(`admin.invites.states.${invite.state}`)}</td>
        <td>{invite.createdBy || '–'}</td>
        <td>{dateTime(invite.expiresAt)}</td>
        <td class="end">
          {#if invite.state === 'pending'}
            <button class="btn" onclick={() => revoke(invite.id)}
              >{t('admin.invites.revoke')}</button
            >
          {/if}
        </td>
      </tr>
    {:else}
      <tr><td colspan="4" class="muted">{t('admin.invites.none')}</td></tr>
    {/each}
  </tbody>
</table>

<style>
  .bar {
    display: flex;
    align-items: end;
    gap: var(--space-2);
    margin-bottom: var(--space-4);
  }
  label {
    display: grid;
    gap: var(--space-1);
  }
  .fresh {
    display: grid;
    gap: var(--space-2);
    justify-items: start;
    max-width: 620px;
    margin-bottom: var(--space-4);
    padding: var(--space-4);
    border: 1px solid var(--color-accent);
    border-radius: var(--radius-popover);
  }
  .fresh p {
    margin: 0;
  }
  code {
    max-width: 100%;
    overflow-wrap: anywhere;
    font-family: var(--font-mono);
    user-select: all;
  }
  table {
    width: 100%;
    max-width: 760px;
    border-collapse: collapse;
  }
  th {
    text-align: left;
    color: var(--color-text-muted);
    font-size: 13px;
  }
  th,
  td {
    padding: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }
  .end {
    text-align: right;
  }
  .problem {
    color: var(--color-danger);
  }
</style>
