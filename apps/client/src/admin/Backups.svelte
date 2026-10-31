<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The instance's backups: where (a folder on the server, S3, WebDAV,
     SFTP), the password, when, what to keep; how the runs went; the backups
     to check, test restore, or bring one person back from. A whole instance
     is restored from the command line, the server stopped: what to keep for
     that is shown here. Setting up starts simple, a folder on the server
     every day and a password; Advanced shows every place and choice. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import { t } from '../lib/i18n.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api } from './api.svelte'
  import { bytes, dateTime, errorText, relative } from './format'

  type Kind = 'folder' | 's3' | 'webdav' | 'sftp'
  type Target = { kind: Kind } & Record<string, string | number | boolean>
  type Policy = { last: number; daily: number; weekly: number; monthly: number; yearly: number }
  type Schedule = 'off' | 'hourly' | 'daily' | 'weekly'
  interface View {
    config: { target: Target; schedule: Schedule; policy: Policy } | null
    status: {
      lastRunAt: number | null
      lastSuccessAt: number | null
      lastError: string | null
      lastReport: { users: number; files: number; size: number } | null
      snapshots: number
      storedBytes: number
    }
    running: boolean
    masterKeyApart: boolean
    suggestedFolder: string
  }
  interface Snapshot {
    id: string
    time: number
    appVersion: string
    counts: Record<string, number>
    size: number
  }
  interface Check {
    snapshots: number
    storedBytes: number
    problems: string[]
  }

  const KINDS: Kind[] = ['folder', 's3', 'webdav', 'sftp']
  const FIELDS: Record<Kind, { key: string; secret?: boolean; area?: boolean; check?: boolean }[]> =
    {
      folder: [{ key: 'path' }],
      s3: [
        { key: 'endpoint' },
        { key: 'region' },
        { key: 'bucket' },
        { key: 'prefix' },
        { key: 'accessKey' },
        { key: 'secretKey', secret: true },
        { key: 'pathStyle', check: true },
      ],
      webdav: [{ key: 'url' }, { key: 'username' }, { key: 'password', secret: true }],
      sftp: [
        { key: 'host' },
        { key: 'port' },
        { key: 'username' },
        { key: 'password', secret: true },
        { key: 'privateKey', secret: true, area: true },
        { key: 'passphrase', secret: true },
        { key: 'path' },
        { key: 'hostKey' },
      ],
    }

  /** `onconfigured` hears whether backups are set up, each time it is known. */
  let { onconfigured }: { onconfigured?: (on: boolean) => void } = $props()

  let view: View | null = $state(null)
  /** Every place and choice, rather than a folder on the server. */
  let advanced = $state(false)
  let problem = $state('')
  let saved = $state('')
  let busy = $state(false)
  let editing = $state(false)
  let kind: Kind = $state('folder')
  let fields: Record<string, string | number | boolean> = $state({})
  let existing = $state(false)
  let password = $state('')
  let again = $state('')
  let schedule: Schedule = $state('daily')
  let policy: Policy = $state({ last: 10, daily: 7, weekly: 4, monthly: 12, yearly: 0 })
  let snapshots: Snapshot[] | null = $state(null)
  let check: Check | null = $state(null)
  let person = $state('')
  let personSnapshot = $state('')

  const offsetSeconds = () => -new Date().getTimezoneOffset() * 60

  async function act(action: () => Promise<void>, done = '') {
    busy = true
    problem = ''
    saved = ''
    try {
      await action()
      // an action may say what it did itself
      if (done) saved = done
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  function show(next: View) {
    view = next
    onconfigured?.(!!next.config)
    if (next.config) {
      schedule = next.config.schedule
      policy = { ...next.config.policy }
    }
  }

  void act(async () => show(await api<View>('GET', '/admin/backups')))

  function edit() {
    const target = view?.config?.target
    kind = target?.kind ?? 'folder'
    fields = target ? { ...target } : {}
    existing = !!target
    password = ''
    again = ''
    advanced = true
    editing = true
  }

  /** The simple setup: a new repository in the suggested folder. */
  function simple() {
    advanced = false
    kind = 'folder'
    fields = { path: view?.suggestedFolder ?? '' }
    existing = false
  }

  $effect(() => {
    if (view && !view.config && !advanced && fields.path === undefined) simple()
  })

  const passwordProblem = $derived.by(() => {
    if (!password) return t('backups.passwordNeeded')
    if (!existing && password.length < 8) return t('backups.passwordShort')
    if (!existing && password !== again) return t('backups.passwordsDiffer')
    return null
  })

  function target(): Target {
    const out: Target = { kind }
    for (const f of FIELDS[kind]) {
      const value = fields[f.key]
      if (f.check) out[f.key] = value === true
      else if (f.key === 'port') out[f.key] = Number(value) || 0
      else out[f.key] = typeof value === 'string' ? value : ''
    }
    return out
  }

  function connect() {
    void act(async () => {
      show(
        await api<View>('PUT', '/admin/backups', {
          target: target(),
          password,
          create: !existing,
          offsetSeconds: offsetSeconds(),
        }),
      )
      editing = false
      snapshots = null
      show(await api<View>('POST', '/admin/backups/run'))
    }, t('admin.backups.connected'))
  }

  function run() {
    if (view) view = { ...view, running: true }
    void act(async () => {
      try {
        show(await api<View>('POST', '/admin/backups/run'))
      } finally {
        show(await api<View>('GET', '/admin/backups'))
      }
      if (snapshots) snapshots = await api<Snapshot[]>('GET', '/admin/backups/snapshots')
    }, t('backups.done'))
  }

  function saveSchedule() {
    void act(async () => {
      show(
        await api<View>('PUT', '/admin/backups/settings', {
          schedule,
          policy,
          offsetSeconds: offsetSeconds(),
        }),
      )
    }, t('backups.saved'))
  }

  function list() {
    void act(async () => {
      snapshots = await api<Snapshot[]>('GET', '/admin/backups/snapshots')
      personSnapshot = snapshots[0]?.id ?? ''
    })
  }

  function runCheck(full: boolean) {
    check = null
    void act(async () => {
      check = await api<Check>('POST', '/admin/backups/check', { full })
    })
  }

  function testRestore(snapshot: Snapshot) {
    void act(async () => {
      const result = await api<{ users: number; files: number; problems: string[] }>(
        'POST',
        '/admin/backups/test-restore',
        { snapshot: snapshot.id },
      )
      if (result.problems.length > 0) throw new Error(result.problems.join(', '))
      saved = t('admin.backups.testOk', { users: result.users, files: result.files })
    })
  }

  async function restorePerson() {
    const when = dateTime(snapshots?.find((s) => s.id === personSnapshot)?.time)
    const yes = await overlay.ask(t('admin.backups.personConfirm', { name: person, when }), {
      confirmLabel: t('backups.restore'),
    })
    if (!yes) return
    void act(async () => {
      const report = await api<{
        account: boolean
        spaces: number
        changes: number
        files: number
      }>('POST', '/admin/backups/restore-person', {
        snapshot: personSnapshot,
        username: person.trim(),
      })
      saved = report.account
        ? t('admin.backups.personBack', { spaces: report.spaces, files: report.files })
        : t('admin.backups.personSpaces', { spaces: report.spaces, files: report.files })
    })
  }

  async function stop() {
    const yes = await overlay.ask(t('admin.backups.stopConfirm'), {
      confirmLabel: t('backups.stop'),
      danger: true,
    })
    if (yes) void act(async () => show(await api<View>('DELETE', '/admin/backups')))
  }

  const recovery = $derived.by(() => {
    const config = (view as View | null)?.config
    return config ? JSON.stringify(config.target, null, 2) : ''
  })
  const SCHEDULES: Schedule[] = ['off', 'hourly', 'daily', 'weekly']
  const KEEP = ['last', 'daily', 'weekly', 'monthly'] as const
</script>

{#if problem}<p class="problem" role="alert" data-backups-problem>{problem}</p>{/if}
{#if saved}<p class="saved" role="status" data-backups-saved>{saved}</p>{/if}

<!-- the simple setup says what it needs in two lines, the rest comes with
     Advanced or once backups run -->
{#if view && (view.config || advanced)}
  <section>
    <p class="muted">{t('admin.backups.help')}</p>
    {#if !view.masterKeyApart}
      <p class="muted small">{t('admin.backups.masterKey')}</p>
    {:else}
      <p class="note">{t('admin.backups.masterKeyApart')}</p>
    {/if}
  </section>
{/if}

{#if view?.config && !editing}
  <section data-backups-status>
    <h2>{t('backups.status')}</h2>
    {#if view.running}
      <p>{t('backups.running')}</p>
    {:else if view.status.lastError}
      <p class="problem">{t('backups.lastFailed', { error: view.status.lastError })}</p>
    {/if}
    <p>
      {view.status.lastSuccessAt
        ? t('backups.lastSuccess', {
            when: relative(view.status.lastSuccessAt),
            date: dateTime(view.status.lastSuccessAt),
          })
        : t('backups.never')}
    </p>
    {#if view.status.snapshots > 0}
      <p class="muted">
        {t('backups.size', { count: view.status.snapshots, size: bytes(view.status.storedBytes) })}
      </p>
    {/if}
    <div class="actions">
      <button class="btn primary" disabled={busy || view.running} onclick={run}
        >{t('backups.now')}</button
      >
      <button class="btn" disabled={busy} onclick={list}>{t('backups.list')}</button>
      <button class="btn" disabled={busy} onclick={() => runCheck(false)}
        >{t('backups.check')}</button
      >
    </div>
    {#if check}
      {#if check.problems.length === 0}
        <p data-backups-check>
          {t('backups.checkOk', { count: check.snapshots, size: bytes(check.storedBytes) })}
        </p>
        <div class="actions">
          <button class="btn" disabled={busy} onclick={() => runCheck(true)}
            >{t('backups.checkFull')}</button
          >
        </div>
      {:else}
        <div class="problem" data-backups-check>
          <p>{t('backups.checkBad')}</p>
          <ul>
            {#each check.problems as p (p)}<li>{p}</li>{/each}
          </ul>
        </div>
      {/if}
    {/if}
  </section>

  {#if snapshots}
    <section data-backups-list>
      <h2>{t('backups.backups')}</h2>
      {#if snapshots.length === 0}
        <p class="muted">{t('backups.none')}</p>
      {:else}
        <ul class="snapshots">
          {#each snapshots as s (s.id)}
            <li data-snapshot={s.id}>
              <span
                ><strong>{dateTime(s.time)}</strong>
                <span class="muted"
                  >{t('admin.backups.counts', {
                    users: s.counts.users ?? 0,
                    files: s.counts.files ?? 0,
                  })}, {bytes(s.size)}</span
                ></span
              >
              <button class="btn" disabled={busy} onclick={() => testRestore(s)}
                >{t('backups.test')}</button
              >
            </li>
          {/each}
        </ul>
        <h2>{t('admin.backups.person')}</h2>
        <p class="muted">{t('admin.backups.personHelp')}</p>
        <div class="actions">
          <label class="field-label"
            >{t('admin.backups.username')}<input class="field" bind:value={person} /></label
          >
          <label class="field-label"
            >{t('admin.backups.from')}<select class="field" bind:value={personSnapshot}>
              {#each snapshots as s (s.id)}<option value={s.id}>{dateTime(s.time)}</option>{/each}
            </select></label
          >
          <button
            class="btn"
            disabled={busy || !person.trim() || !personSnapshot}
            onclick={() => void restorePerson()}>{t('backups.restore')}</button
          >
        </div>
      {/if}
    </section>
  {/if}

  <section>
    <h2>{t('backups.when')}</h2>
    <label class="field-label"
      >{t('backups.schedule')}<select class="field" bind:value={schedule}>
        {#each SCHEDULES as s (s)}<option value={s}>{t(`backups.schedule_${s}`)}</option>{/each}
      </select></label
    >
    <div class="actions">
      {#each KEEP as period (period)}
        <label class="field-label keep"
          >{t(`backups.keep_${period}`)}<input
            class="field"
            type="number"
            min="0"
            max="100"
            bind:value={policy[period]}
          /></label
        >
      {/each}
    </div>
    <p class="muted small">{t('backups.keepHelp')}</p>
    <div class="actions">
      <button class="btn" disabled={busy} onclick={saveSchedule}>{t('backups.save')}</button>
    </div>
  </section>

  <section>
    <h2>{t('admin.backups.recovery')}</h2>
    <p class="muted">{t('admin.backups.recoveryHelp')}</p>
    <pre data-backups-recovery>{recovery}</pre>
    <div class="actions">
      <button class="btn" disabled={busy} onclick={edit}>{t('backups.change')}</button>
      <button class="btn danger" disabled={busy} onclick={() => void stop()}
        >{t('backups.stop')}</button
      >
    </div>
  </section>
{:else if view}
  <form
    data-backups-form
    onsubmit={(e) => {
      e.preventDefault()
      if (!passwordProblem) connect()
    }}
  >
    <h2>{view.config ? t('backups.change') : t('backups.setUp')}</h2>
    {#if !advanced}
      <p class="muted">{t('admin.backups.simpleHelp')}</p>
      <label class="field-label"
        >{t('admin.backups.field_path')}<input
          class="field wide"
          value={String(fields.path ?? '')}
          oninput={(e) => (fields.path = e.currentTarget.value)}
          autocomplete="off"
        /></label
      >
      <p class="muted small">{t('admin.backups.simpleWhere')}</p>
    {:else}
      <fieldset>
        <legend>{t('backups.placeKind')}</legend>
        {#each KINDS as k (k)}
          <label class="choice"
            ><input type="radio" name="kind" value={k} bind:group={kind} />{t(
              `admin.backups.kind_${k}`,
            )}</label
          >
        {/each}
      </fieldset>
      {#each FIELDS[kind] as f (f.key)}
        {#if f.check}
          <label class="choice"
            ><input
              type="checkbox"
              checked={fields[f.key] === true}
              onchange={(e) => (fields[f.key] = e.currentTarget.checked)}
            />{t(`admin.backups.field_${f.key}`)}</label
          >
        {:else}
          <label class="field-label"
            >{t(`admin.backups.field_${f.key}`)}
            {#if f.area}
              <textarea
                class="field"
                rows="4"
                value={String(fields[f.key] ?? '')}
                oninput={(e) => (fields[f.key] = e.currentTarget.value)}
                placeholder={view.config ? t('backups.unchanged') : ''}></textarea>
            {:else}
              <input
                class="field"
                type={f.secret ? 'password' : 'text'}
                value={String(fields[f.key] ?? '')}
                oninput={(e) => (fields[f.key] = e.currentTarget.value)}
                placeholder={f.secret && view.config ? t('backups.unchanged') : ''}
                autocomplete="off"
              />
            {/if}
          </label>
        {/if}
      {/each}
      {#if kind === 'sftp'}<p class="muted small">{t('admin.backups.hostKeyHelp')}</p>{/if}
      <fieldset>
        <legend>{t('backups.repository')}</legend>
        <label class="choice"
          ><input type="radio" name="existing" value={false} bind:group={existing} />{t(
            'backups.new',
          )}</label
        >
        <label class="choice"
          ><input type="radio" name="existing" value={true} bind:group={existing} />{t(
            'backups.existing',
          )}</label
        >
      </fieldset>
    {/if}
    <label class="field-label"
      >{existing ? t('backups.password') : t('backups.newPassword')}<PasswordField
        class="field"
        bind:value={password}
        autocomplete="new-password"
      /></label
    >
    {#if !existing}
      <label class="field-label"
        >{t('backups.passwordAgain')}<PasswordField
          eye={false}
          class="field"
          bind:value={again}
          autocomplete="new-password"
        /></label
      >
      <p class="muted small">{t('backups.passwordWarning')}</p>
    {/if}
    {#if passwordProblem && password}<p class="problem">{passwordProblem}</p>{/if}
    <div class="actions">
      <button class="btn primary" type="submit" disabled={busy || !!passwordProblem}
        >{busy ? t('backups.connecting') : t('backups.start')}</button
      >
      {#if view.config}
        <button type="button" class="btn" disabled={busy} onclick={() => (editing = false)}
          >{t('backups.cancel')}</button
        >
      {/if}
    </div>
    {#if !view.config}
      <button
        type="button"
        class="link"
        onclick={() => (advanced ? simple() : (advanced = true))}
        data-backups-advanced
        >{advanced ? t('admin.backups.backToSimple') : t('admin.backups.advanced')}</button
      >
    {/if}
  </form>
{/if}

<style>
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-accent);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    text-decoration: underline;
  }
  .field.wide {
    width: min(480px, 100%);
  }
  section,
  form {
    display: grid;
    gap: var(--space-3);
    margin-bottom: var(--space-6);
    justify-items: start;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  p {
    margin: 0;
  }
  .small {
    font-size: 12px;
  }
  .note {
    font-weight: 600;
  }
  fieldset {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: var(--space-1);
    font-weight: 600;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .field-label {
    display: grid;
    gap: var(--space-1);
    width: min(420px, 100%);
    font-weight: 600;
  }
  .field-label .field {
    font-weight: 400;
  }
  .keep {
    width: 8em;
  }
  textarea {
    height: auto;
    font-family: var(--font-mono);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: var(--space-3);
  }
  .snapshots {
    justify-self: stretch;
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .snapshots li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  .snapshots .muted {
    margin-left: var(--space-2);
    font-size: 12px;
  }
  pre {
    justify-self: stretch;
    margin: 0;
    padding: var(--space-3);
    overflow-x: auto;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
    font-size: 12px;
  }
  .problem {
    color: var(--color-danger);
  }
  .problem ul {
    margin: var(--space-1) 0 0;
    padding-left: var(--space-5);
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .saved {
    color: var(--color-success);
  }
  .saved,
  .problem {
    margin-bottom: var(--space-4);
  }
</style>
