<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Scheduled backups of this device's notes. A first setup is a folder and
     a password; once on, the page shows how the last runs went, Back up now
     and Restore. The rest waits under Advanced: S3 and WebDAV, a backup
     already there, when they run and what they keep, checks, test restores,
     changing where, stopping. Below, a backup in one file made or restored
     by hand (`onbackup` opens its dialog). Settings groups, see app.css. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import { onMount } from 'svelte'
  import { backend } from '../lib/backend'
  import type {
    BackupCheck,
    BackupPolicy,
    BackupSchedule,
    BackupSnapshot,
    BackupTarget,
  } from '../lib/backend/types'
  import { backupError, backups } from '../lib/backups.svelte'
  import { bytes, dateTime, relative } from '../lib/format'
  import { t } from '../lib/i18n.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { pickToOpen } from '../lib/pick'
  import { workspace } from '../lib/workspace.svelte'

  /** Opens the dialog that makes or restores a backup in one file. */
  let { onbackup }: { onbackup?: (mode: 'make' | 'restore') => void } = $props()

  type Kind = BackupTarget['kind']

  const view = $derived(backups.view)
  const config = $derived(view?.config ?? null)

  // the form: shown to set up, and to change where
  let editing = $state(false)
  let kind: Kind = $state('folder')
  /** A phone names its own backup folder: there is nothing to pick there. */
  const phoneFolder = window.AppSystem?.backupFolder?.() ?? ''
  let folder = $state(phoneFolder)
  let s3 = $state({
    endpoint: '',
    region: '',
    bucket: '',
    prefix: '',
    accessKey: '',
    secretKey: '',
    pathStyle: false,
  })
  let webdav = $state({ url: '', username: '', password: '' })
  let existing = $state(false)
  let password = $state('')
  let again = $state('')
  /** On a device whose notes are not encrypted, the backup password is
   * optional: left empty, a fixed one stands in, so the backups are as open
   * as the notes they copy, and restoring works with the field empty too. */
  let openDevice = $state(false)
  const NO_PASSWORD = 'ficelle-backup-without-password'
  let host = $state('')

  /** Everything past a folder and a password, see the comment on top. */
  let advanced = $state(false)
  let busy = $state(false)
  let message: string | null = $state(null)
  let failure: string | null = $state(null)
  let schedule: BackupSchedule = $state('daily')
  let policy: BackupPolicy = $state({ last: 10, daily: 7, weekly: 4, monthly: 12, yearly: 0 })
  let snapshots: BackupSnapshot[] | null = $state(null)
  let check: BackupCheck | null = $state(null)

  onMount(() => {
    void backend.vaultStatus().then((v) => (openDevice = !v.encrypted))
    void backups.refresh().then(() => {
      if (!config && view) kind = view.kinds[0] ?? 'folder'
    })
  })

  // what is saved shows in the schedule's fields
  $effect(() => {
    if (config) {
      schedule = config.schedule
      policy = { ...config.policy }
    }
  })

  function errorText(err: unknown): string {
    const e = err as { kind?: string; message?: string }
    return backupError(e?.kind, e?.message ?? String(err))
  }

  async function act(action: () => Promise<void>, done?: string) {
    busy = true
    message = null
    failure = null
    try {
      await action()
      if (done) message = done
    } catch (err) {
      failure = errorText(err)
    } finally {
      busy = false
    }
  }

  function edit() {
    const target = config?.target
    if (target) {
      kind = target.kind
      if (target.kind === 'folder') folder = target.path
      if (target.kind === 's3') s3 = { ...target }
      if (target.kind === 'webdav') webdav = { ...target }
      host = config?.host ?? ''
    }
    // a place already used holds a backup: its password, not a new one
    existing = !!config
    password = ''
    again = ''
    editing = true
  }

  function target(): BackupTarget {
    if (kind === 'folder') return { kind, path: folder.trim() }
    if (kind === 's3') return { kind, ...s3 }
    return { kind, ...webdav }
  }

  const passwordProblem = $derived.by(() => {
    if (openDevice && !password && !again) return null
    if (!password) return t('backups.passwordNeeded')
    if (!existing && password.length < 8) return t('backups.passwordShort')
    if (!existing && password !== again) return t('backups.passwordsDiffer')
    return null
  })

  /** The short setup: a new backup in a folder. */
  function connectFolder() {
    kind = 'folder'
    existing = false
    connect()
  }

  function connect() {
    void act(async () => {
      const used = password || (openDevice ? NO_PASSWORD : password)
      backups.view = await backend.backupsConnect(target(), used, !existing, host)
      password = ''
      again = ''
      editing = false
      snapshots = null
      // the first backup at once, so the person sees it work
      await backups.run()
    }, t('backups.connected'))
  }

  async function chooseFolder() {
    const picked = await pickToOpen({ directory: true, title: t('backups.chooseFolder') })
    if (picked) folder = picked
  }

  function saveSchedule() {
    void act(async () => {
      backups.view = await backend.backupsSettings(schedule, policy)
    }, t('backups.saved'))
  }

  function runNow() {
    void act(async () => {
      await backups.run()
      if (snapshots) snapshots = await backend.backupsSnapshots()
    }, t('backups.done'))
  }

  function list() {
    void act(async () => {
      snapshots = await backend.backupsSnapshots()
    })
  }

  function runCheck(full: boolean) {
    check = null
    void act(async () => {
      check = await backend.backupsCheck(full)
    })
  }

  function testRestore(snapshot: BackupSnapshot) {
    void act(async () => {
      const result = await backend.backupsTestRestore(snapshot.id)
      if (result.problems.length > 0) throw new Error(result.problems.join(', '))
      message = t('backups.testOk', {
        what: `${t('backups.notes', { count: result.restored.notes })}, ${t('backups.files', {
          count: result.restored.files,
        })}`,
      })
    })
  }

  async function restore(snapshot: BackupSnapshot) {
    const yes = await overlay.ask(t('backups.restoreConfirm', { when: dateTime(snapshot.time) }), {
      confirmLabel: t('backups.restore'),
    })
    if (!yes) return
    void act(async () => {
      const report = await backend.backupsRestore(snapshot.id)
      await workspace.load()
      message = t('backups.restored', { notes: report.notes, skipped: report.skipped })
    })
  }

  async function disconnect() {
    const yes = await overlay.ask(t('backups.stopConfirm'), {
      confirmLabel: t('backups.stop'),
      danger: true,
    })
    if (!yes) return
    void act(async () => {
      backups.view = await backend.backupsDisconnect()
      snapshots = null
    })
  }

  function where(): string {
    const target = config?.target
    if (!target) return ''
    if (target.kind === 'folder') return target.path
    if (target.kind === 's3') {
      const host = target.endpoint
        ? target.endpoint.replace(/^https?:\/\//, '').replace(/\/$/, '')
        : `AWS ${target.region}`
      return `${target.bucket}${target.prefix ? `/${target.prefix}` : ''}, ${host}`
    }
    return target.url
  }

  const SCHEDULES: BackupSchedule[] = ['off', 'hourly', 'daily', 'weekly']
  const KEEP = ['last', 'daily', 'weekly', 'monthly'] as const
</script>

<div class="settings-group">
  <p class="settings-help">{t('backups.help')}</p>
  {#if !backend.native}
    <p class="settings-help">{t('errors.unsupported')}</p>
  {/if}
</div>

{#if view && config && !editing}
  <div class="settings-group" data-backups-status>
    <h3>{t('backups.status')}</h3>
    <p class="where">{t('backups.where', { where: where() })}</p>
    {#if view.running}
      <p class="state">{t('backups.running')}</p>
    {:else if view.status.lastError}
      <p class="state bad" role="alert">
        {t('backups.lastFailed', {
          error: backupError(view.status.lastErrorKind, view.status.lastError),
        })}
      </p>
    {/if}
    <p class="state">
      {view.status.lastSuccessAt
        ? t('backups.lastSuccess', {
            when: relative(view.status.lastSuccessAt),
            date: dateTime(view.status.lastSuccessAt),
          })
        : t('backups.never')}
    </p>
    {#if view.status.snapshots > 0}
      <p class="settings-help">
        {t('backups.size', {
          count: view.status.snapshots,
          size: bytes(view.status.storedBytes),
        })}
      </p>
    {/if}
    <div class="settings-actions">
      <button class="btn primary" disabled={busy || view.running} onclick={runNow}
        >{t('backups.now')}</button
      >
      <button class="btn" disabled={busy} onclick={list}>{t('backups.restoreList')}</button>
    </div>
  </div>

  {#if check}
    <div class="settings-group" data-backups-check>
      {#if check.problems.length === 0}
        <p class="state">
          {t('backups.checkOk', { count: check.snapshots, size: bytes(check.storedBytes) })}
        </p>
        <div class="settings-actions">
          <button class="btn" disabled={busy} onclick={() => runCheck(true)}
            >{t('backups.checkFull')}</button
          >
        </div>
      {:else}
        <p class="state bad" role="alert">{t('backups.checkBad')}</p>
        <ul class="problems">
          {#each check.problems as problem (problem)}<li>{problem}</li>{/each}
        </ul>
      {/if}
    </div>
  {/if}

  {#if snapshots}
    <div class="settings-group wide" data-backups-list>
      <h3>{t('backups.backups')}</h3>
      {#if snapshots.length === 0}
        <p class="settings-help">{t('backups.none')}</p>
      {:else}
        <ul class="list">
          {#each snapshots as snapshot (snapshot.id)}
            <li class="snapshot" data-snapshot={snapshot.id}>
              <div class="name">
                <strong>{dateTime(snapshot.time)}</strong>
                <span class="muted"
                  >{t('backups.notes', { count: snapshot.counts.notes ?? 0 })}, {t(
                    'backups.files',
                    { count: snapshot.counts.files ?? 0 },
                  )}{snapshot.host !== config.host ? `, ${snapshot.host}` : ''}</span
                >
              </div>
              <div class="settings-actions">
                <button class="btn" disabled={busy} onclick={() => testRestore(snapshot)}
                  >{t('backups.test')}</button
                >
                <button class="btn" disabled={busy} onclick={() => void restore(snapshot)}
                  >{t('backups.restore')}</button
                >
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}

  <details class="advanced" bind:open={advanced}>
    <summary>{t('backups.advanced')}</summary>
    <div class="settings-group">
      <h3>{t('backups.checkTitle')}</h3>
      <div class="settings-actions">
        <button class="btn" disabled={busy} onclick={() => runCheck(false)}
          >{t('backups.check')}</button
        >
      </div>
    </div>

    <div class="settings-group">
      <h3>{t('backups.when')}</h3>
      <label class="settings-field">
        {t('backups.schedule')}
        <select class="field" bind:value={schedule}>
          {#each SCHEDULES as s (s)}<option value={s}>{t(`backups.schedule_${s}`)}</option>{/each}
        </select>
      </label>
      <p class="settings-help">{t('backups.whenHelp')}</p>
      <div class="keep">
        {#each KEEP as period (period)}
          <label class="settings-field">
            {t(`backups.keep_${period}`)}
            <input class="field" type="number" min="0" max="100" bind:value={policy[period]} />
          </label>
        {/each}
      </div>
      <p class="settings-help">{t('backups.keepHelp')}</p>
      <div class="settings-actions">
        <button class="btn" disabled={busy} onclick={saveSchedule}>{t('backups.save')}</button>
      </div>
    </div>

    <div class="settings-group">
      <h3>{t('backups.place')}</h3>
      <div class="settings-actions">
        <button class="btn" disabled={busy} onclick={edit}>{t('backups.change')}</button>
        <button class="btn danger" disabled={busy} onclick={() => void disconnect()}
          >{t('backups.stop')}</button
        >
      </div>
    </div>
  </details>
{:else if view && !config && !advanced && view.kinds.includes('folder')}
  <form
    class="settings-group"
    data-backups-simple
    onsubmit={(e) => {
      e.preventDefault()
      if (!passwordProblem && folder.trim()) connectFolder()
    }}
  >
    <h3>{t('backups.setUp')}</h3>
    {#if phoneFolder}
      <p class="settings-help">{t('backups.phoneFolder')}</p>
      <code class="phone-folder">{phoneFolder}</code>
    {:else}
      <label class="settings-field">
        {t('backups.folder')}
        <span class="row">
          <input class="field" bind:value={folder} autocomplete="off" />
          <button type="button" class="btn" onclick={() => void chooseFolder()}
            >{t('backups.chooseFolder')}</button
          >
        </span>
      </label>
    {/if}
    <label class="settings-field">
      {t('backups.newPassword')}
      <PasswordField class="field" bind:value={password} autocomplete="new-password" />
    </label>
    <label class="settings-field">
      {t('backups.passwordAgain')}
      <PasswordField eye={false} class="field" bind:value={again} autocomplete="new-password" />
    </label>
    <p class="settings-help warn">
      {openDevice ? t('backups.passwordOptional') : t('backups.passwordWarning')}
    </p>
    {#if passwordProblem && password && (again || password.length < 8)}
      <p class="settings-help bad">{passwordProblem}</p>
    {/if}
    <div class="settings-actions">
      <button
        class="btn primary"
        type="submit"
        disabled={busy || !!passwordProblem || !folder.trim()}
        >{busy ? t('backups.connecting') : t('backups.turnOn')}</button
      >
    </div>
    <p class="settings-help">{t('backups.simpleWhen')}</p>
  </form>
  <button class="link" type="button" onclick={() => (advanced = true)}
    >{t('backups.advanced')}</button
  >
{:else if view}
  <form
    class="settings-group"
    data-backups-form
    onsubmit={(e) => {
      e.preventDefault()
      if (!passwordProblem) connect()
    }}
  >
    <h3>{config ? t('backups.change') : t('backups.setUp')}</h3>
    <fieldset class="kinds">
      <legend>{t('backups.placeKind')}</legend>
      {#each view.kinds as k (k)}
        <label class="settings-check">
          <input type="radio" name="backup-kind" value={k} bind:group={kind} />
          {t(`backups.kind_${k}`)}
        </label>
      {/each}
    </fieldset>

    {#if kind === 'folder'}
      <label class="settings-field">
        {t('backups.folder')}
        <span class="row">
          <input class="field" bind:value={folder} autocomplete="off" />
          <button type="button" class="btn" onclick={() => void chooseFolder()}
            >{t('backups.chooseFolder')}</button
          >
        </span>
      </label>
      <p class="settings-help">{t('backups.folderHelp')}</p>
    {:else if kind === 's3'}
      <label class="settings-field"
        >{t('backups.endpoint')}<input
          class="field"
          bind:value={s3.endpoint}
          placeholder="https://s3.eu-central-003.backblazeb2.com"
          autocomplete="off"
        /></label
      >
      <p class="settings-help">{t('backups.endpointHelp')}</p>
      <label class="settings-field"
        >{t('backups.region')}<input
          class="field"
          bind:value={s3.region}
          placeholder="us-east-1"
          autocomplete="off"
        /></label
      >
      <label class="settings-field"
        >{t('backups.bucket')}<input
          class="field"
          bind:value={s3.bucket}
          autocomplete="off"
        /></label
      >
      <label class="settings-field"
        >{t('backups.prefix')}<input
          class="field"
          bind:value={s3.prefix}
          autocomplete="off"
        /></label
      >
      <label class="settings-field"
        >{t('backups.accessKey')}<input
          class="field"
          bind:value={s3.accessKey}
          autocomplete="off"
        /></label
      >
      <label class="settings-field"
        >{t('backups.secretKey')}<PasswordField
          class="field"
          bind:value={s3.secretKey}
          placeholder={config ? t('backups.unchanged') : ''}
          autocomplete="off"
        /></label
      >
      <label class="settings-check">
        <input type="checkbox" bind:checked={s3.pathStyle} />
        {t('backups.pathStyle')}
      </label>
    {:else}
      <label class="settings-field"
        >{t('backups.url')}<input
          class="field"
          bind:value={webdav.url}
          placeholder="https://cloud.example.com/remote.php/dav/files/me/Backups"
          autocomplete="off"
        /></label
      >
      <label class="settings-field"
        >{t('backups.username')}<input
          class="field"
          bind:value={webdav.username}
          autocomplete="off"
        /></label
      >
      <label class="settings-field"
        >{t('backups.davPassword')}<PasswordField
          class="field"
          bind:value={webdav.password}
          placeholder={config ? t('backups.unchanged') : ''}
          autocomplete="off"
        /></label
      >
    {/if}

    <fieldset class="kinds">
      <legend>{t('backups.repository')}</legend>
      <label class="settings-check">
        <input type="radio" name="backup-existing" value={false} bind:group={existing} />
        {t('backups.new')}
      </label>
      <label class="settings-check">
        <input type="radio" name="backup-existing" value={true} bind:group={existing} />
        {t('backups.existing')}
      </label>
    </fieldset>
    <label class="settings-field">
      {existing ? t('backups.password') : t('backups.newPassword')}
      <PasswordField class="field" bind:value={password} autocomplete="new-password" />
    </label>
    {#if !existing}
      <label class="settings-field">
        {t('backups.passwordAgain')}
        <PasswordField eye={false} class="field" bind:value={again} autocomplete="new-password" />
      </label>
      <p class="settings-help warn">
        {openDevice ? t('backups.passwordOptional') : t('backups.passwordWarning')}
      </p>
    {/if}
    <label class="settings-field">
      {t('backups.host')}
      <input class="field" bind:value={host} placeholder={t('backups.hostDefault')} />
    </label>
    {#if passwordProblem && password && (existing || again || password.length < 8)}
      <p class="settings-help bad">{passwordProblem}</p>
    {/if}
    <div class="settings-actions">
      <button class="btn primary" type="submit" disabled={busy || !!passwordProblem}
        >{busy ? t('backups.connecting') : t('backups.start')}</button
      >
      {#if config}
        <button type="button" class="btn" disabled={busy} onclick={() => (editing = false)}
          >{t('backups.cancel')}</button
        >
      {:else if view.kinds.includes('folder')}
        <button type="button" class="btn" disabled={busy} onclick={() => (advanced = false)}
          >{t('backups.simple')}</button
        >
      {/if}
    </div>
  </form>
{/if}

{#if failure}<p class="message bad" role="alert" data-backups-failure>{failure}</p>{/if}
{#if message}<p class="message" role="status" data-backups-message>{message}</p>{/if}

{#if onbackup}
  <div class="settings-group" data-backup-file>
    <h3>{t('backup.fileTitle')}</h3>
    <p class="settings-help">{t('backup.help')}</p>
    <div class="settings-actions">
      <button class="btn" onclick={() => onbackup('make')} disabled={!backend.native}
        >{t('backup.makeTitle')}</button
      >
      <button class="btn" onclick={() => onbackup('restore')} disabled={!backend.native}
        >{t('backup.restoreTitle')}</button
      >
    </div>
  </div>
{/if}

<style>
  .phone-folder {
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .advanced {
    display: grid;
    gap: var(--space-4);
  }
  .advanced summary {
    cursor: pointer;
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .advanced[open] summary {
    margin-bottom: var(--space-2);
  }
  .link {
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font: inherit;
    font-size: 13px;
  }
  .link:hover {
    text-decoration: underline;
  }
  .where {
    margin: 0;
    overflow-wrap: anywhere;
    font-family: var(--font-mono);
    font-size: 13px;
  }
  .state,
  .message {
    margin: 0;
  }
  .bad {
    color: var(--color-danger);
  }
  .warn {
    color: var(--color-text);
  }
  .list {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .snapshot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
  }
  .name {
    display: grid;
    gap: 2px;
  }
  .name .muted {
    font-size: 12px;
  }
  .problems {
    margin: 0;
    padding-left: var(--space-5);
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--color-danger);
  }
  .kinds {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    border: 0;
  }
  .kinds legend {
    margin-bottom: var(--space-2);
    font-weight: 600;
  }
  .keep {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .keep .field {
    width: 6em;
  }
  .row {
    display: flex;
    gap: var(--space-2);
  }
  .row .field {
    flex: 1;
  }
</style>
