<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Import from another app: pick the app (with how to get its export),
     pick the export, see what it holds and what will not come in as it
     was, then write it into a new folder and say how it went here, what
     did not come in as it was listed on demand.
     `ficelle` opens it for this app's own export (Settings, Data, Export, a
     folder or its zip), read by the markdown importer, with no app to pick. -->
<script lang="ts">
  import { pickToOpen } from '../lib/pick'
  import { brand } from '@app/brand'
  import { onDestroy } from 'svelte'
  import { backend } from '../lib/backend'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { closeConverter } from '../lib/import/html'
  import { closeSource, IMPORTERS, openSource } from '../lib/import/index'
  import {
    detect,
    plainSnippet,
    prepare,
    write,
    type Prepared,
    type Written,
  } from '../lib/import/pipeline'
  import type { Importer, ImportSource, ReportKind } from '../lib/import/types'
  import { modal } from '../lib/modal'
  import { sync } from '../lib/sync.svelte'
  import { isBackendError, workspace } from '../lib/workspace.svelte'

  let { close, ficelle = false }: { close: () => void; ficelle?: boolean } = $props()

  type Stage = 'choose' | 'reading' | 'preview' | 'writing' | 'done'
  let stage = $state<Stage>('choose')
  // svelte-ignore state_referenced_locally
  let importer = $state<Importer>(
    (ficelle ? IMPORTERS.find((i) => i.id === 'markdown') : IMPORTERS[0]) as Importer,
  )
  let source: ImportSource | null = null
  let prepared = $state<Prepared | null>(null)
  let written = $state<Written | null>(null)
  let folderName = $state('')
  let progress = $state({ done: 0, total: 0 })
  let error = $state<string | null>(null)
  let notice = $state<string | null>(null)
  let stop: AbortController | null = null

  const KINDS: ReportKind[] = [
    'linkMissing',
    'fileMissing',
    'fileLeftOut',
    'dropped',
    'encrypted',
    'simplified',
    'unreadable',
  ]

  const errorText = (err: unknown) => t(`errors.${isBackendError(err) ? err.kind : 'unknown'}`)

  const day = (time: number) =>
    new Intl.DateTimeFormat(currentLocale(), { dateStyle: 'medium' }).format(time)

  async function finishSource() {
    if (source) await closeSource(source).catch(() => {})
    source = null
    closeConverter()
  }

  onDestroy(() => {
    stop?.abort()
    void finishSource()
  })

  async function choose(kind: 'folder' | 'file') {
    error = null
    notice = null
    const picked = await pickToOpen(
      kind === 'folder'
        ? { directory: true, title: t('import.chooseFolder') }
        : {
            title: t('import.chooseFile'),
            filters: [{ name: importer.app, extensions: importer.extensions }],
          },
    )
    if (picked === null) return
    stage = 'reading'
    progress = { done: 0, total: 0 }
    try {
      await finishSource()
      source = await openSource(picked)
      if (importer.detect(source) === 0) {
        const other = detect(source, IMPORTERS)
        if (!other) throw { kind: 'notAnExport', message: 'no importer reads this' }
        notice = t('import.otherFormat', { picked: importer.app, found: other.app })
        importer = other
      }
      prepared = await prepare(source, importer, backend, (done, total) => {
        progress = { done, total }
      })
      folderName = t('import.folderName', {
        app: ficelle ? brand.displayName : importer.app,
        date: day(Date.now()),
      })
      stage = 'preview'
    } catch (err) {
      error = errorText(err)
      stage = 'choose'
      await finishSource()
    }
  }

  async function start() {
    if (!prepared) return
    stage = 'writing'
    stop = new AbortController()
    progress = { done: 0, total: prepared.counts.notes }
    try {
      written = await write(prepared, backend, {
        name: folderName.trim() || importer.app,
        signal: stop.signal,
        progress: (done, total) => (progress = { done, total }),
      })
      stage = 'done'
      await Promise.all([workspace.refreshFolders(), workspace.refreshTagsAndNotes()])
    } catch (err) {
      error = errorText(err)
      stage = 'preview'
    } finally {
      stop = null
      await finishSource()
    }
  }

  const reportCounts = $derived(
    KINDS.map((kind) => ({
      kind,
      count: (written?.report ?? prepared?.report ?? []).filter((r) => r.kind === kind).length,
    })).filter((k) => k.count > 0),
  )
  const topFolders = $derived(
    prepared ? prepared.model.folders.filter((f) => f.parent === null).map((f) => f.name) : [],
  )
  const sample = $derived(
    prepared ? prepared.model.notes.filter((n) => !prepared?.known.has(n.key)).slice(0, 3) : [],
  )
  // Android's folder picker gives an address no file can be opened under:
  // exports come as a file there (a zip of a folder)
  const android = /android/i.test(navigator.userAgent)
  const busy = $derived(stage === 'reading' || stage === 'writing')
  const closeIfIdle = () => !busy && close()
</script>

<div
  class="backdrop"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && closeIfIdle()}
>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label={t('import.title')}
    use:modal={{ onclose: closeIfIdle }}
  >
    <header>
      <h2>{ficelle ? t('import.ficelleTitle', { app: brand.displayName }) : t('import.title')}</h2>
      <button class="icon-btn" aria-label={t('common.close')} disabled={busy} onclick={close}
        ><Icon name="close" /></button
      >
    </header>

    {#if stage === 'choose'}
      {#if ficelle}
        <p class="muted how">{t('import.ficelleHow', { app: brand.displayName })}</p>
      {:else}
        <label class="app">
          <span>{t('import.from')}</span>
          <select class="field" bind:value={importer} data-autofocus>
            {#each IMPORTERS as option (option.id)}
              <option value={option}>{option.app}</option>
            {/each}
          </select>
        </label>
        <p class="muted how">{t(`import.how.${importer.id}`)}</p>
      {/if}
      <div class="actions">
        {#if importer.picks.includes('folder') && !android}
          <button class="btn" onclick={() => choose('folder')}
            ><Icon name="folder" size={16} />{t('import.chooseFolder')}</button
          >
        {/if}
        {#if importer.picks.includes('file')}
          <button class="btn" onclick={() => choose('file')}
            ><Icon name="export" size={16} />{t('import.chooseFile')}</button
          >
        {/if}
      </div>
    {:else if stage === 'reading'}
      <p aria-live="polite">{t('import.reading')}</p>
      <progress max={progress.total || 1} value={progress.done}></progress>
    {:else if stage === 'preview' && prepared}
      {#if notice}<p class="muted">{notice}</p>{/if}
      <dl class="counts">
        <dt>{t('import.notes')}</dt>
        <dd>{prepared.counts.notes}</dd>
        <dt>{t('import.folders')}</dt>
        <dd>{prepared.counts.folders}</dd>
        <dt>{t('import.files')}</dt>
        <dd>{prepared.counts.files}</dd>
        <dt>{t('import.tags')}</dt>
        <dd>{prepared.counts.tags}</dd>
        {#if prepared.dates}
          <dt>{t('import.dates')}</dt>
          <dd>{day(prepared.dates.from)} – {day(prepared.dates.to)}</dd>
        {/if}
      </dl>
      {#if prepared.counts.skipped}
        <p class="muted">
          {t('import.skipped', { count: prepared.counts.skipped })}
          {#if sync.status.state !== 'off'}{t('import.skippedHere')}{/if}
        </p>
      {/if}
      {#if topFolders.length}
        <p class="muted small folders">
          {topFolders.slice(0, 12).join(', ')}{topFolders.length > 12 ? '…' : ''}
        </p>
      {/if}
      {#if sample.length}
        <ul class="sample">
          {#each sample as note (note.key)}
            <li>
              <strong>{note.title || t('note.untitled')}</strong>
              <span class="muted small">{plainSnippet(note.body)}</span>
            </li>
          {/each}
        </ul>
      {/if}
      {#if reportCounts.length}
        <div class="warnings">
          <p>{t('import.warnings')}</p>
          <ul>
            {#each reportCounts as item (item.kind)}
              <li>{t(`import.kind.${item.kind}`)}: {item.count}</li>
            {/each}
          </ul>
        </div>
      {/if}
      {#if prepared.counts.notes}
        <label class="app">
          <span>{t('import.into')}</span>
          <input class="field" bind:value={folderName} />
        </label>
      {:else}
        <p>{t('import.nothing')}</p>
      {/if}
    {:else if stage === 'writing'}
      <p aria-live="polite">
        {t('import.writing', { done: progress.done, total: progress.total })}
      </p>
      <progress max={progress.total || 1} value={progress.done}></progress>
    {:else if stage === 'done' && written}
      <p role="status">
        {written.cancelled
          ? t('import.stopped', { count: written.notes })
          : t('import.done', { count: written.notes, folder: folderName })}
      </p>
      {#if written.skipped}
        <p class="muted">{t('import.alreadyHere', { count: written.skipped })}</p>
      {/if}
      {#if reportCounts.length}
        <ul>
          {#each reportCounts as item (item.kind)}
            <li>{t(`import.kind.${item.kind}`)}: {item.count}</li>
          {/each}
        </ul>
        <details class="report">
          <summary>{t('import.showWhich')}</summary>
          <ul>
            {#each written.report.slice(0, 200) as item, i (i)}
              <li>
                {#if item.note}<b>{item.note}</b>{/if}
                {item.detail ?? ''}
                <span class="muted">{t(`import.kind.${item.kind}`)}</span>
              </li>
            {/each}
          </ul>
        </details>
      {:else if !written.cancelled}
        <p class="muted">{t('import.reportNone')}</p>
      {/if}
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="actions">
      <span class="grow"></span>
      {#if stage === 'preview' && prepared}
        <button
          class="btn"
          onclick={() => ((stage = 'choose'), (prepared = null), void finishSource())}
          >{t('common.back')}</button
        >
        {#if prepared.counts.notes}
          <button class="btn primary" onclick={start}
            >{t('import.start', { count: prepared.counts.notes })}</button
          >
        {:else}
          <button class="btn primary" onclick={close}>{t('common.close')}</button>
        {/if}
      {:else if stage === 'writing'}
        <button class="btn" onclick={() => stop?.abort()}>{t('import.stop')}</button>
      {:else if !busy}
        <button class="btn" onclick={close}>{t('common.close')}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .report ul {
    max-height: 200px;
    overflow: auto;
    margin: var(--space-2) 0 0;
    padding-left: var(--space-4);
    overflow-wrap: anywhere;
  }
  .report summary {
    cursor: pointer;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    width: min(520px, 100%);
    max-height: calc(100vh - 2 * var(--space-4));
    overflow-y: auto;
    display: grid;
    gap: var(--space-3);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 16px;
  }
  p {
    margin: 0;
  }
  .small {
    font-size: 12px;
  }
  .how {
    white-space: pre-line;
  }
  .app {
    display: grid;
    gap: var(--space-1);
  }
  .counts {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-1) var(--space-4);
    margin: 0;
  }
  .counts dt {
    color: var(--color-text-muted);
  }
  .counts dd {
    margin: 0;
  }
  .folders {
    overflow-wrap: anywhere;
  }
  .sample {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: var(--space-2);
  }
  .sample li {
    display: grid;
    min-width: 0;
  }
  .sample span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .warnings ul {
    margin: var(--space-1) 0 0;
    padding-left: var(--space-5);
  }
  progress {
    width: 100%;
  }
  .error {
    color: var(--color-danger);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .grow {
    flex: 1;
  }
</style>
