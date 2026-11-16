<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Attachments with the server: what the account uses of its storage,
     and files the server refused. Shown under the sync settings once
     connected. -->
<script lang="ts">
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { backend, type FilesInfo } from '../lib/backend'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import { sync } from '../lib/sync.svelte'

  interface RefusedFile {
    name: string
    note: string
    reason: string
  }

  let info = $state<FilesInfo | null>(null)
  let refused = $state<RefusedFile[]>([])

  function bytes(n: number): string {
    const units = ['kilobyte', 'megabyte', 'gigabyte', 'terabyte'] as const
    let value = n / 1024
    let unit = 0
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024
      unit += 1
    }
    return new Intl.NumberFormat(currentLocale(), {
      style: 'unit',
      unit: units[unit],
      unitDisplay: 'short',
      maximumFractionDigits: value < 10 ? 1 : 0,
    }).format(value)
  }

  // after each round: the usage moves, a refusal may be new
  $effect(() => {
    void sync.status.lastSyncAt
    void backend
      .filesInfo()
      .then((next) => (info = next))
      .catch(() => {})
    void invoke<RefusedFile[]>('files_refused')
      .then((next) => (refused = next))
      .catch(() => {})
  })

  const share = $derived(
    info && info.policy.quotaBytes > 0
      ? Math.min(100, Math.round((info.usage.usedBytes / info.policy.quotaBytes) * 100))
      : null,
  )
</script>

{#if info}
  <Group title={t('files.title')}>
    <Row
      label={info.policy.quotaBytes > 0
        ? t('files.usedOf', {
            used: bytes(info.usage.usedBytes),
            quota: bytes(info.policy.quotaBytes),
          })
        : t('files.used', { used: bytes(info.usage.usedBytes) })}
    >
      {#if share !== null}
        <span
          class="meter"
          class:full={share >= 90}
          role="meter"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={share}
          aria-label={t('files.title')}><span style:width="{share}%"></span></span
        >
      {/if}
    </Row>
  </Group>
  {#if refused.length > 0}
    <p role="alert">{t('files.refused', { count: refused.length })}</p>
    <ul class="refused">
      {#each refused as file (file.note + file.name)}
        <li>
          <b>{file.name}</b>
          <span class="muted"
            >{file.note || t('note.untitled')}, {t(`files.reason.${file.reason}`)}</span
          >
        </li>
      {/each}
    </ul>
  {/if}
{/if}

<style>
  .meter {
    display: block;
    height: 6px;
    width: 140px;
    border-radius: var(--radius-chip);
    background: var(--color-border);
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
  .refused {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .refused li {
    display: grid;
    padding: var(--space-1) 0;
  }
</style>
