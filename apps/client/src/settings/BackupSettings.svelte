<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Scheduled backups of this device's notes, in two tabs. Basic: before
     any, two ways in, start backing up or restore from backups made before,
     each a folder first, then what that folder holds decides the next step;
     once on, how the last runs went, Back up now, the list to restore
     from, and a backup in one file made or restored by hand (`onbackup`
     opens its dialog). Advanced: S3 and WebDAV, a backup already there,
     when they run and what they keep, checks, changing where, stopping.
     Settings groups, see app.css. -->
<script module lang="ts">
  type Tab = 'basic' | 'advanced'
  /** The tab chosen last, kept while the app runs. */
  let lastTab: Tab = 'basic'
</script>

<script lang="ts">
  import { MIN_PASSWORD } from '../lib/passwords'
  import PasswordField from '../lib/PasswordField.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
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

  const uid = $props.id()
  const TABS: Tab[] = ['basic', 'advanced']
  let tab: Tab = $state(lastTab)
  const tabButtons: Record<Tab, HTMLButtonElement | undefined> = $state({
    basic: undefined,
    advanced: undefined,
  })

  function select(next: Tab, focus = false) {
    tab = next
    lastTab = next
    if (focus) tabButtons[next]?.focus()
  }

  /** Arrows move between the tabs, Home and End to the ends. */
  function tabKey(e: KeyboardEvent) {
    const at = TABS.indexOf(tab)
    const to =
      e.key === 'ArrowRight' || e.key === 'ArrowDown'
        ? (at + 1) % TABS.length
        : e.key === 'ArrowLeft' || e.key === 'ArrowUp'
          ? (at + TABS.length - 1) % TABS.length
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? TABS.length - 1
              : -1
    if (to < 0) return
    e.preventDefault()
    select(TABS[to] ?? tab, true)
  }

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
  /** The simple setup's way in: starting backups, or restoring from some
   * made before. None yet: the two are offered. */
  let mode: 'start' | 'restore' | null = $state(null)
  /** The simple setup's folder already holds backups: it opens them, to
   * restore from, never a second set over them. */
  let found = $state(false)
  /** Those backups have a password of their own. */
  let needsPassword = $state(true)
  /** The folder that was looked into, null while looking or before. */
  let looked: string | null = $state(null)
  let looking = $state(false)
  /** Starting, with backups already in the folder: they are used. */
  let using = $state(false)
  let password = $state('')
  let again = $state('')
  /** On a device whose notes are not encrypted, the backup password is
   * optional: left empty, a fixed one stands in (NO_PASSWORD on the native
   * side), so the backups are as open as the notes they copy. */
  let openDevice = $state(false)
  let host = $state('')
  let listElement: HTMLElement | undefined = $state()

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
      // a phone's folder is fixed: a reinstalled app finds its old backups
      if (!config) void look()
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

  /** The backup takes the notes' own password, typed once and checked:
   * offered only where the notes have one. */
  let sameAsNotes = $state(false)

  const passwordProblem = $derived.by(() => {
    if (sameAsNotes && !existing && !found) return password ? null : t('backups.passwordNeeded')
    if (openDevice && !password && !again) return null
    if (!password) return t('backups.passwordNeeded')
    // backups found here have their password already, nothing to choose
    const choosing = !existing && !found
    if (choosing && password.length < MIN_PASSWORD) return t('backups.passwordShort')
    if (choosing && password !== again) return t('backups.passwordsDiffer')
    return null
  })

  /** What blocks the simple setup's button. */
  const simpleProblem = $derived(
    found ? (needsPassword && !password ? t('backups.passwordNeeded') : null) : passwordProblem,
  )

  /** What the simple setup's folder holds: backups or not, with a password
   * or not. Looked at as soon as the folder is chosen. */
  async function look() {
    const path = folder.trim()
    // the same folder again (its field left after a change): already known
    if (path && path === looked) return
    using = false
    looked = null
    if (!path) {
      found = false
      return
    }
    looking = true
    try {
      const there = await backend.backupsFound({ kind: 'folder', path })
      found = there.found
      needsPassword = there.needsPassword
    } catch {
      found = false
      needsPassword = true
    } finally {
      looking = false
      looked = path
    }
  }

  function choose(next: 'start' | 'restore' | null) {
    mode = next
    using = false
    password = ''
    again = ''
    failure = null
    message = null
  }

  function connectFolder() {
    kind = 'folder'
    existing = found
    if (sameAsNotes && !found) {
      // the notes' password, checked before a backup is made with it
      void (async () => {
        failure = null
        const ok = await backend.vaultCheckPassword(password).catch(() => false)
        if (ok) void connect()
        else failure = t('backups.notesPasswordWrong')
      })()
      return
    }
    const restoring = mode === 'restore'
    void connect().then(() => {
      if (restoring && !failure && snapshots && snapshots.length > 0) void offerLatest()
    })
  }

  function connect(): Promise<void> {
    const opening = existing
    return act(
      async () => {
        // empty: backups without a password, see openDevice
        backups.view = await backend.backupsConnect(target(), password, !existing, host)
        password = ''
        again = ''
        editing = false
        found = false
        mode = null
        // the status and the list are on the first tab
        select('basic')
        if (opening) {
          // backups from before: their list, to restore from, and no new one
          // yet, it would be the newest and hold only what is here now
          snapshots = await backend.backupsSnapshots()
          return
        }
        snapshots = null
        // the first backup at once, so the person sees it work
        await backups.run()
      },
      opening ? t('backups.foundOpened') : t('backups.connected'),
    )
  }

  async function chooseFolder() {
    const picked = await pickToOpen({ directory: true, title: t('backups.chooseFolder') })
    if (picked) {
      folder = picked
      await look()
    }
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

  /** Right after backups made before are opened: the newest is likely the
   * one wanted, offered at once, the list stays for another. */
  async function offerLatest() {
    const latest = snapshots?.reduce((a, b) => (b.time > a.time ? b : a))
    if (!latest) return
    const answer = await overlay.askOr(
      t('backups.latestAsk', {
        when: dateTime(latest.time),
        notes: t('backups.notes', { count: latest.counts.notes ?? 0 }),
      }),
      { confirmLabel: t('backups.latestRestore'), altLabel: t('backups.latestOther') },
    )
    if (answer === 'confirm') restoreNow(latest)
    else if (answer === 'alt') listElement?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  }

  async function restore(snapshot: BackupSnapshot) {
    const yes = await overlay.ask(t('backups.restoreConfirm', { when: dateTime(snapshot.time) }), {
      confirmLabel: t('backups.restore'),
    })
    if (yes) restoreNow(snapshot)
  }

  /** Restores without asking: asked already. */
  function restoreNow(snapshot: BackupSnapshot) {
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
      check = null
      select('basic')
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

  /** The status card's headline: what is true of the backups now. */
  const health = $derived.by((): { icon: IconName; tone: string; text: string } | null => {
    if (!view) return null
    const status = view.status
    if (view.running) return { icon: 'syncing', tone: 'busy', text: t('backups.running') }
    if (status.lastError) return { icon: 'warning', tone: 'bad', text: t('backups.failedTitle') }
    if (status.lastSuccessAt)
      return {
        icon: 'success',
        tone: 'good',
        text: t('backups.backedUp', { when: relative(status.lastSuccessAt) }),
      }
    // backups opened from before: none made here yet, but they are there
    const newest = snapshots?.reduce((t0, sn) => Math.max(t0, sn.time), 0) ?? 0
    if (newest)
      return {
        icon: 'success',
        tone: 'good',
        text: t('backups.newestFound', { when: relative(newest) }),
      }
    return { icon: 'info', tone: 'quiet', text: t('backups.never') }
  })

  const SCHEDULES: BackupSchedule[] = ['off', 'hourly', 'daily', 'weekly']
  const KEEP = ['last', 'daily', 'weekly', 'monthly'] as const
</script>

{#snippet notices()}
  {#if failure}<p class="notice bad" role="alert" data-backups-failure>
      <Icon name="warning" size={16} />{failure}
    </p>{/if}
  {#if message}<p class="notice" role="status" data-backups-message>
      <Icon name="success" size={16} />{message}
    </p>{/if}
{/snippet}

{#snippet oneFile()}
  {#if onbackup}
    <div class="settings-group" data-backup-file>
      <h3>{t('backup.fileTitle')}</h3>
      <p class="settings-help">{t('backup.help')}</p>
      <div class="settings-actions fill">
        <button class="btn" onclick={() => onbackup('make')} disabled={!backend.native}
          ><Icon name="export" size={16} />{t('backup.makeTitle')}</button
        >
        <button class="btn" onclick={() => onbackup('restore')} disabled={!backend.native}
          ><Icon name="import" size={16} />{t('backup.restoreTitle')}</button
        >
      </div>
    </div>
  {/if}
{/snippet}

<div class="backups">
  <div class="intro">
    <span class="tile"><Icon name="backup" size={20} /></span>
    <div class="intro-text">
      <p class="settings-help">{t('backups.help')}</p>
      {#if !backend.native}<p class="settings-help">{t('errors.unsupported')}</p>{/if}
    </div>
  </div>

  {#if view}
    <div class="tabs" role="tablist" aria-label={t('backups.title')}>
      {#each TABS as name (name)}
        <button
          bind:this={tabButtons[name]}
          type="button"
          role="tab"
          id="{uid}-tab-{name}"
          aria-selected={tab === name}
          aria-controls="{uid}-panel-{name}"
          tabindex={tab === name ? 0 : -1}
          class:selected={tab === name}
          onclick={() => select(name)}
          onkeydown={tabKey}>{t(`backups.${name}`)}</button
        >
      {/each}
    </div>

    <div
      class="panel"
      role="tabpanel"
      id="{uid}-panel-{tab}"
      aria-labelledby="{uid}-tab-{tab}"
      tabindex="-1"
    >
      {#if tab === 'basic'}
        {#if config && health}
          <section class="card" data-backups-status>
            <div class="headline">
              <span class="state-icon {health.tone}"><Icon name={health.icon} size={22} /></span>
              <div class="headline-text">
                <h3>{health.text}</h3>
                {#if view.status.lastError && !view.running}
                  <p class="bad" role="alert">
                    {backupError(view.status.lastErrorKind, view.status.lastError)}
                  </p>
                {/if}
                {#if view.status.lastError && !view.running}
                  <!-- the last one that went through, said with its age -->
                  <p class="muted">
                    {view.status.lastSuccessAt
                      ? t('backups.lastSuccess', {
                          when: relative(view.status.lastSuccessAt),
                          date: dateTime(view.status.lastSuccessAt),
                        })
                      : t('backups.never')}
                  </p>
                {:else if view.status.lastSuccessAt}
                  <p class="muted">{dateTime(view.status.lastSuccessAt)}</p>
                {/if}
              </div>
            </div>
            <dl class="facts">
              {#if view.status.snapshots > 0}
                <div>
                  <dt>{t('backups.backups')}</dt>
                  <dd>
                    {t('backups.size', {
                      count: view.status.snapshots,
                      size: bytes(view.status.storedBytes),
                    })}
                  </dd>
                </div>
              {/if}
              <div>
                <dt>{t('backups.place')}</dt>
                <dd class="where">{where()}</dd>
              </div>
            </dl>
            <div class="settings-actions fill">
              <button class="btn primary" disabled={busy || view.running} onclick={runNow}
                ><Icon name="backup" size={16} />{t('backups.now')}</button
              >
              <button class="btn" disabled={busy} onclick={list}
                ><Icon name="restore" size={16} />{t('backups.restoreList')}</button
              >
            </div>
          </section>

          {@render notices()}

          {#if snapshots}
            <div class="settings-group wide" data-backups-list bind:this={listElement}>
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
                      <div class="row-actions">
                        <button
                          class="btn quiet"
                          disabled={busy}
                          onclick={() => testRestore(snapshot)}>{t('backups.test')}</button
                        >
                        <button
                          class="btn quiet"
                          disabled={busy}
                          onclick={() => void restore(snapshot)}>{t('backups.restore')}</button
                        >
                      </div>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/if}
        {:else if !config && view.kinds.includes('folder')}
          {#if !mode}
            <div class="choices" data-backups-choice>
              <button type="button" class="choice" onclick={() => choose('start')}>
                <span class="state-icon quiet"><Icon name="backup" size={22} /></span>
                <span class="choice-text">
                  <strong>{t('backups.startTitle')}</strong>
                  <span class="muted">{t('backups.simpleWhen')}</span>
                </span>
                <Icon name="right" size={18} />
              </button>
              <button type="button" class="choice" onclick={() => choose('restore')}>
                <span class="state-icon quiet"><Icon name="restore" size={22} /></span>
                <span class="choice-text">
                  <strong>{t('backups.restoreFrom')}</strong>
                  <span class="muted">{t('backups.restoreFromHelp')}</span>
                </span>
                <Icon name="right" size={18} />
              </button>
            </div>
          {:else}
            <form
              class="card"
              data-backups-simple
              data-mode={mode}
              onsubmit={(e) => {
                e.preventDefault()
                if (looking || looked !== folder.trim() || simpleProblem) return
                if (found ? mode === 'restore' || using : mode === 'start') connectFolder()
              }}
            >
              <button type="button" class="back" onclick={() => choose(null)}
                ><Icon name="back" size={16} />{t('common.back')}</button
              >
              <div class="headline">
                <span class="state-icon quiet"
                  ><Icon name={mode === 'restore' ? 'restore' : 'backup'} size={22} /></span
                >
                <div class="headline-text">
                  <h3>{mode === 'restore' ? t('backups.restoreFrom') : t('backups.setUp')}</h3>
                  {#if mode === 'start' || !phoneFolder}
                    <p class="muted">
                      {mode === 'restore'
                        ? t('backups.restoreFolderHelp')
                        : t('backups.simpleWhen')}
                    </p>
                  {/if}
                </div>
              </div>

              {#if phoneFolder}
                <p class="settings-help">
                  {mode === 'restore' ? t('backups.phoneFolderRestore') : t('backups.phoneFolder')}
                </p>
                <code class="phone-folder">{phoneFolder}</code>
              {:else}
                <label class="settings-field">
                  {t('backups.folder')}
                  <span class="row">
                    <input
                      class="field"
                      bind:value={folder}
                      autocomplete="off"
                      onchange={() => void look()}
                    />
                    <button
                      type="button"
                      class="btn"
                      class:primary={!folder.trim()}
                      onclick={() => void chooseFolder()}>{t('backups.chooseFolder')}</button
                    >
                  </span>
                </label>
              {/if}

              {#if looking}
                <p class="notice quiet" role="status">
                  <Icon name="syncing" size={16} />{t('backups.looking')}
                </p>
              {:else if looked !== null && looked === folder.trim()}
                {#if found}
                  <p class="notice" role="status">
                    <Icon name="success" size={16} />{t('backups.foundTitle')}
                  </p>
                  {#if mode === 'start' && !using}
                    <p class="settings-help">{t('backups.foundStartHelp')}</p>
                    <div class="settings-actions fill">
                      <button type="button" class="btn primary" onclick={() => (using = true)}
                        >{t('backups.useThem')}</button
                      >
                      {#if !phoneFolder}
                        <button type="button" class="btn" onclick={() => void chooseFolder()}
                          >{t('backups.chooseAnother')}</button
                        >
                      {/if}
                    </div>
                  {:else}
                    {#if needsPassword}
                      <label class="settings-field">
                        {t('backups.password')}
                        <PasswordField
                          class="field"
                          bind:value={password}
                          autocomplete="current-password"
                        />
                      </label>
                    {:else}
                      <p class="settings-help">{t('backups.noPasswordNeeded')}</p>
                    {/if}
                    <div class="settings-actions fill">
                      <button class="btn primary" type="submit" disabled={busy || !!simpleProblem}
                        >{busy ? t('backups.connecting') : t('backups.foundOpen')}</button
                      >
                    </div>
                  {/if}
                {:else if mode === 'restore'}
                  <p class="notice bad" role="status">
                    <Icon name="warning" size={16} />{t('backups.noneHere')}
                  </p>
                  {#if !phoneFolder}
                    <p class="settings-help">{t('backups.noneHereHelp')}</p>
                  {/if}
                {:else}
                  {#if sameAsNotes}
                    <label class="settings-check">
                      <input type="checkbox" bind:checked={sameAsNotes} />
                      {t('backups.sameAsNotes')}
                    </label>
                    <label class="settings-field">
                      {t('backups.notesPassword')}
                      <PasswordField
                        class="field"
                        bind:value={password}
                        autocomplete="current-password"
                      />
                    </label>
                    <p class="settings-help warn">{t('backups.sameAsNotesHelp')}</p>
                  {:else}
                    {#if !openDevice}
                      <label class="settings-check">
                        <input type="checkbox" bind:checked={sameAsNotes} />
                        {t('backups.sameAsNotes')}
                      </label>
                    {/if}
                    <div class="pair">
                      <label class="settings-field">
                        {t('backups.newPassword')}
                        <PasswordField
                          class="field"
                          bind:value={password}
                          autocomplete="new-password"
                        />
                      </label>
                      <label class="settings-field">
                        {t('backups.passwordAgain')}
                        <PasswordField
                          eye={false}
                          class="field"
                          bind:value={again}
                          autocomplete="new-password"
                        />
                      </label>
                    </div>
                    <p class="settings-help warn">
                      {openDevice ? t('backups.passwordOptional') : t('backups.passwordWarning')}
                    </p>
                  {/if}
                  {#if passwordProblem && password && (again || password.length < MIN_PASSWORD)}
                    <p class="settings-help bad">{passwordProblem}</p>
                  {/if}
                  <div class="settings-actions fill">
                    <button class="btn primary" type="submit" disabled={busy || !!passwordProblem}
                      >{busy ? t('backups.connecting') : t('backups.turnOn')}</button
                    >
                  </div>
                {/if}
              {/if}
            </form>
          {/if}
          {@render notices()}
        {:else if !config}
          <!-- no folder on this device: S3 and WebDAV are set up under Advanced -->
          <div class="settings-group">
            <div class="settings-actions">
              <button class="btn primary" onclick={() => select('advanced', true)}
                >{t('backups.setUp')}</button
              >
            </div>
          </div>
          {@render notices()}
        {/if}

        {@render oneFile()}
      {:else if config && !editing}
        <div class="settings-group">
          <h3>{t('backups.when')}</h3>
          <label class="settings-field">
            {t('backups.schedule')}
            <select class="field" bind:value={schedule}>
              {#each SCHEDULES as s (s)}<option value={s}>{t(`backups.schedule_${s}`)}</option
                >{/each}
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
          <div class="settings-actions fill">
            <button class="btn" disabled={busy} onclick={saveSchedule}>{t('backups.save')}</button>
          </div>
        </div>

        <div class="settings-group wide">
          <h3>{t('backups.checkTitle')}</h3>
          <div class="settings-actions fill">
            <button class="btn" disabled={busy} onclick={() => runCheck(false)}
              >{t('backups.check')}</button
            >
            {#if check && check.problems.length === 0}
              <button class="btn" disabled={busy} onclick={() => runCheck(true)}
                >{t('backups.checkFull')}</button
              >
            {/if}
          </div>
          {#if check}
            <div class="check" data-backups-check>
              {#if check.problems.length === 0}
                <p class="notice">
                  <Icon name="success" size={16} />{t('backups.checkOk', {
                    count: check.snapshots,
                    size: bytes(check.storedBytes),
                  })}
                </p>
              {:else}
                <p class="notice bad" role="alert">
                  <Icon name="warning" size={16} />{t('backups.checkBad')}
                </p>
                <ul class="problems">
                  {#each check.problems as problem (problem)}<li>{problem}</li>{/each}
                </ul>
              {/if}
            </div>
          {/if}
        </div>

        <div class="settings-group wide">
          <h3>{t('backups.place')}</h3>
          <p class="where">{where()}</p>
          <div class="settings-actions fill">
            <button class="btn" disabled={busy} onclick={edit}>{t('backups.change')}</button>
            <button class="btn danger" disabled={busy} onclick={() => void disconnect()}
              >{t('backups.stop')}</button
            >
          </div>
        </div>
        {@render notices()}
      {:else}
        <form
          class="settings-group"
          data-backups-form
          onsubmit={(e) => {
            e.preventDefault()
            if (!passwordProblem) void connect()
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
              <PasswordField
                eye={false}
                class="field"
                bind:value={again}
                autocomplete="new-password"
              />
            </label>
            <p class="settings-help warn">
              {openDevice ? t('backups.passwordOptional') : t('backups.passwordWarning')}
            </p>
          {/if}
          <label class="settings-field">
            {t('backups.host')}
            <input class="field" bind:value={host} placeholder={t('backups.hostDefault')} />
          </label>
          {#if passwordProblem && password && (existing || again || password.length < MIN_PASSWORD)}
            <p class="settings-help bad">{passwordProblem}</p>
          {/if}
          <div class="settings-actions fill">
            <button class="btn primary" type="submit" disabled={busy || !!passwordProblem}
              >{busy ? t('backups.connecting') : t('backups.start')}</button
            >
            {#if config}
              <button type="button" class="btn" disabled={busy} onclick={() => (editing = false)}
                >{t('backups.cancel')}</button
              >
            {:else if view.kinds.includes('folder')}
              <button
                type="button"
                class="btn"
                disabled={busy}
                onclick={() => select('basic', true)}>{t('backups.simple')}</button
              >
            {/if}
          </div>
        </form>
        {@render notices()}
      {/if}
    </div>
  {:else}
    {@render oneFile()}
  {/if}
</div>

<style>
  .backups {
    display: grid;
    align-content: start;
    gap: var(--space-5);
    min-width: 0;
  }
  .backups p {
    margin: 0;
  }
  .intro-text {
    display: grid;
    gap: var(--space-1);
  }
  .intro {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }
  .tile {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: var(--radius-control);
    background: var(--color-surface-2);
    color: var(--color-accent-text);
  }

  /* the tab bar, drawn like the settings' segmented choices */
  .tabs {
    display: inline-flex;
    justify-self: start;
    gap: var(--space-1);
    padding: 3px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface);
  }
  .tabs [role='tab'] {
    min-width: 96px;
    height: 28px;
    padding: 0 var(--space-4);
    border: 1px solid transparent;
    border-radius: var(--radius-chip);
    background: transparent;
    color: var(--color-text-muted);
    font-weight: 500;
  }
  .tabs [role='tab'].selected {
    border-color: var(--color-accent);
    background: var(--color-surface-2);
    color: var(--color-text);
  }
  @media (hover: hover) {
    .tabs [role='tab']:not(.selected):hover {
      color: var(--color-text);
      background: var(--color-surface-2);
    }
  }
  .tabs [role='tab']:focus-visible {
    outline-offset: 1px;
  }
  .panel {
    display: grid;
    align-content: start;
    gap: var(--space-5);
    min-width: 0;
  }
  .panel:focus-visible {
    outline: none;
  }

  /* the status, and the first setup, as one card */
  .card {
    display: grid;
    gap: var(--space-4);
    min-width: 0;
    margin: 0;
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  .card p {
    margin: 0;
  }
  .card .field {
    width: min(320px, 100%);
  }
  .headline {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }
  .headline-text {
    display: grid;
    gap: var(--space-1);
    min-width: 0;
  }
  .headline h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }
  .headline .muted {
    font-size: 13px;
    line-height: 1.45;
  }
  .state-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: var(--radius-control);
    background: var(--color-surface-2);
    color: var(--color-text-muted);
  }
  .state-icon.good {
    color: var(--color-success);
  }
  .state-icon.bad {
    color: var(--color-danger);
  }
  .state-icon.busy {
    color: var(--color-accent-text);
  }
  .facts {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
  }
  .facts div {
    display: grid;
    grid-template-columns: 7em 1fr;
    gap: var(--space-3);
  }
  .facts dt {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .facts dd {
    margin: 0;
    min-width: 0;
    font-size: 13px;
  }
  .where {
    margin: 0;
    overflow-wrap: anywhere;
    font-family: var(--font-mono);
    font-size: 13px;
  }
  .phone-folder {
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .notice {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    font-size: 13px;
    line-height: 1.45;
  }
  .notice :global(.icon) {
    margin-top: 1px;
    color: var(--color-success);
  }
  .notice.bad :global(.icon) {
    color: var(--color-danger);
  }
  .notice.quiet,
  .notice.quiet :global(.icon) {
    color: var(--color-text-muted);
  }

  /* the two ways in, before any backup */
  .choices {
    display: grid;
    gap: var(--space-3);
  }
  .choice {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
    color: var(--color-text);
    text-align: left;
    transition: border-color var(--dur-fast) var(--ease-out);
  }
  @media (hover: hover) {
    .choice:hover {
      border-color: var(--color-border-strong);
      background: var(--color-surface-2);
    }
  }
  .choice > :global(.icon) {
    color: var(--color-text-muted);
  }
  .choice-text {
    display: grid;
    flex: 1;
    gap: var(--space-1);
    min-width: 0;
  }
  .choice-text strong {
    font-size: 15px;
  }
  .choice-text .muted {
    font-size: 13px;
    line-height: 1.45;
  }
  .back {
    display: inline-flex;
    align-items: center;
    justify-self: start;
    gap: var(--space-1);
    margin: calc(-1 * var(--space-2)) 0 0 calc(-1 * var(--space-2));
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--color-text-muted);
    font-size: 13px;
  }
  @media (hover: hover) {
    .back:hover {
      color: var(--color-text);
      background: var(--color-surface-2);
    }
  }
  .bad {
    color: var(--color-danger);
  }
  .warn {
    color: var(--color-text);
  }

  /* the backups to restore from, as rows of one list */
  .list {
    display: grid;
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  .snapshot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-4);
  }
  .snapshot + .snapshot {
    border-top: 1px solid var(--color-border);
  }
  .name {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .name .muted {
    font-size: 12px;
  }
  .row-actions {
    display: flex;
    flex: none;
    gap: var(--space-1);
  }
  .btn.quiet {
    border-color: transparent;
    background: transparent;
    color: var(--color-text-muted);
  }
  @media (hover: hover) {
    .btn.quiet:hover:not(:disabled) {
      border-color: var(--color-border);
      background: var(--color-surface-2);
      color: var(--color-text);
    }
  }

  .check {
    display: grid;
    gap: var(--space-2);
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
    display: grid;
    grid-template-columns: repeat(4, max-content);
    gap: var(--space-3);
  }
  .keep .field {
    width: 6em;
  }
  .pair {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .pair .settings-field {
    flex: 1 1 220px;
    justify-items: stretch;
  }
  .pair :global(.field) {
    width: 100%;
  }
  .row {
    display: flex;
    gap: var(--space-2);
    width: min(440px, 100%);
  }
  .row .field {
    flex: 1;
    min-width: 0;
  }

  /* touch screens: tabs and buttons across the width, rows stacked */
  @media (pointer: coarse) {
    .tabs {
      display: flex;
      justify-self: stretch;
    }
    .tabs [role='tab'] {
      flex: 1;
      height: 38px;
      font-size: 15px;
    }
    .fill {
      width: 100%;
    }
    .fill .btn {
      flex: 1 1 0;
      min-width: max-content;
    }
    .card .field,
    .card .settings-field,
    .card :global(.password),
    .row {
      width: 100%;
    }
    .card .settings-field {
      justify-items: stretch;
    }
    .keep {
      grid-template-columns: repeat(2, 1fr);
      width: 100%;
    }
    .keep .field {
      width: 100%;
    }
    .snapshot {
      flex-wrap: wrap;
      padding: var(--space-3);
    }
    .row-actions {
      width: 100%;
    }
    .row-actions .btn {
      flex: 1;
    }
    .btn.quiet {
      border-color: var(--color-border);
    }
  }
</style>
