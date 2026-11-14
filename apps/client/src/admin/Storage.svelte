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

{#if problem}<p class="ad-problem" role="alert">{problem}</p>{/if}
{#if !overview && !problem}<p class="muted" aria-busy="true">{t('admin.loading')}</p>{/if}
{#if overview}
  <div class="ad-strip">
    <div>
      <span class="k">{t('admin.storage.stored')}</span>
      <span class="v">{bytes(overview.storedBytes)}</span>
    </div>
    <div>
      <span class="k">{t('admin.storage.files')}</span>
      <span class="v">{overview.files}</span>
    </div>
    <div>
      <span class="k">{t('admin.storage.notes')}</span>
      <span class="v">{bytes(overview.noteBytes)}</span>
    </div>
    <div>
      <span class="k">{t('admin.storage.saved')}</span>
      <span class="v">{bytes(Math.max(0, overview.referencedBytes - overview.uniqueBytes))}</span>
    </div>
  </div>

  {#if overview.people.total > 1}
    <section class="ad-group" aria-labelledby="storage-people">
      <h2 id="storage-people">{t('admin.storage.people')}</h2>
      <div class="ad-table-box">
        <table class="ad-table">
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
                  {person.quotaBytes
                    ? t('admin.storage.ofQuota', {
                        used: bytes(person.usedBytes),
                        quota: bytes(person.quotaBytes),
                      })
                    : bytes(person.usedBytes)}
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
      </div>
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
    </section>
  {/if}

  <Policy path="/admin/policy" level="instance" onsaved={load} />
{/if}

<style>
  .meter {
    display: block;
    height: 4px;
    margin-top: 4px;
    max-width: 200px;
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
  }
</style>
