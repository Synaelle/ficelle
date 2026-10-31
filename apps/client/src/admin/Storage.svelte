<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The server's storage, the files policy of the whole server, and what
     each person uses against their quota. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { api, type StorageOverview } from './api.svelte'
  import { bytes, errorText } from './format'
  import Policy from './Policy.svelte'

  const PAGE = 50
  let overview: StorageOverview | null = $state(null)
  let offset = $state(0)
  let problem = $state('')

  async function load() {
    problem = ''
    try {
      overview = await api<StorageOverview>('GET', `/admin/storage?offset=${offset}&limit=${PAGE}`)
    } catch (err) {
      problem = errorText(err)
    }
  }

  /** Share of the quota used, 0 to 100, `null` without a quota. */
  const share = (used: number, quota: number) =>
    quota === 0 ? null : Math.min(100, Math.round((used / quota) * 100))

  void load()
</script>

{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
{#if overview}
  <dl class="counts">
    <div>
      <dt>{t('admin.storage.stored')}</dt>
      <dd>{bytes(overview.storedBytes)}</dd>
    </div>
    <div>
      <dt>{t('admin.storage.files')}</dt>
      <dd>{overview.files}</dd>
    </div>
    <div>
      <dt>{t('admin.storage.saved')}</dt>
      <dd>{bytes(Math.max(0, overview.referencedBytes - overview.uniqueBytes))}</dd>
    </div>
    <div>
      <dt>{t('admin.storage.notes')}</dt>
      <dd>{bytes(overview.noteBytes)}</dd>
    </div>
  </dl>

  <h2>{t('admin.storage.people')}</h2>
  <table>
    <thead>
      <tr>
        <th>{t('admin.username')}</th>
        <th>{t('admin.storage.used')}</th>
        <th class="num">{t('admin.storage.files')}</th>
        <th class="num">{t('admin.storage.largest')}</th>
      </tr>
    </thead>
    <tbody>
      {#each overview.people.items as person (person.userId)}
        {@const part = share(person.usedBytes, person.quotaBytes)}
        <tr>
          <td><b>{person.username}</b> <span class="muted">{person.displayName}</span></td>
          <td>
            <span
              >{person.quotaBytes
                ? t('admin.storage.ofQuota', {
                    used: bytes(person.usedBytes),
                    quota: bytes(person.quotaBytes),
                  })
                : bytes(person.usedBytes)}</span
            >
            {#if part !== null}
              <span
                class="meter"
                class:full={part >= 90}
                role="meter"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={part}
                aria-label={t('admin.storage.used')}><span style:width="{part}%"></span></span
              >
            {/if}
          </td>
          <td class="num">{person.files}</td>
          <td class="num">{person.largestBytes ? bytes(person.largestBytes) : '–'}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if overview.people.total > PAGE}
    <div class="pages">
      <button class="btn" disabled={offset === 0} onclick={() => ((offset -= PAGE), load())}
        >{t('admin.previous')}</button
      >
      <button
        class="btn"
        disabled={offset + PAGE >= overview.people.total}
        onclick={() => ((offset += PAGE), load())}>{t('admin.next')}</button
      >
    </div>
  {/if}

  <h2>{t('admin.storage.policy')}</h2>
  <Policy path="/admin/policy" level="instance" onsaved={load} />
{/if}

<style>
  .counts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-3);
    margin: 0 0 var(--space-4);
  }
  .counts div {
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  dt {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  dd {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
  }
  h2 {
    margin: var(--space-5) 0 var(--space-2);
    font-size: 16px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    text-align: left;
    font-weight: 600;
    color: var(--color-text-muted);
    font-size: 13px;
  }
  th,
  td {
    padding: var(--space-2);
    border-bottom: 1px solid var(--color-border);
    vertical-align: top;
  }
  .num {
    text-align: right;
    white-space: nowrap;
  }
  .meter {
    display: block;
    height: 6px;
    margin-top: 4px;
    max-width: 220px;
    border-radius: 999px;
    background: var(--color-surface-2);
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    background: var(--color-accent);
  }
  .meter.full span {
    background: var(--color-danger);
  }
  .pages {
    display: flex;
    gap: var(--space-3);
    margin-top: var(--space-3);
  }
  .problem {
    color: var(--color-danger);
  }
</style>
