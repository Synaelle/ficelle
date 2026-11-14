<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The instance's backups: where (a folder on the server, S3, WebDAV,
     SFTP), the password, when, what to keep; how the runs went; the backups
     to check, test restore, or bring one person back from. A whole instance
     is restored from the command line, the server stopped: what to keep for
     that is shown here. Setting up starts simple, a folder on the server
     every day and a password; Advanced shows every place and choice. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import Icon from '../lib/icons/Icon.svelte'
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
    suggestedApart: boolean
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
  /** Where backups go, in a few words: the folder, bucket, address or host. */
  const where = $derived.by(() => {
    const target = (view as View | null)?.config?.target
    if (!target) return ''
    const field = { folder: 'path', s3: 'bucket', webdav: 'url', sftp: 'host' }[target.kind]
    return String(target[field] ?? '')
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
  <ul class="ad-rows" data-backups-status>
    <li
      class={['ad-row', view.status.lastError ? 'fail' : view.status.lastSuccessAt ? 'ok' : 'warn']}
    >
      <span class="ad-lead"
        ><Icon
          name={view.status.lastError || !view.status.lastSuccessAt ? 'warning' : 'check'}
          size={18}
        /></span
      >
      <span class="ad-words">
        <span class="ad-label">
          {#if view.running}
            {t('backups.running')}
          {:else if view.status.lastSuccessAt}
            {t('backups.lastSuccess', {
              when: relative(view.status.lastSuccessAt),
              date: dateTime(view.status.lastSuccessAt),
            })}
          {:else}
            {t('backups.never')}
          {/if}
        </span>
        {#if view.status.lastError}
          <span class="ad-help ad-bad"
            >{t('backups.lastFailed', { error: view.status.lastError })}</span
          >
        {/if}
        <span class="ad-help">
          {#if where}{t('backups.where', { where })}{/if}
          {#if view.status.snapshots > 0}
            {t('backups.size', {
              count: view.status.snapshots,
              size: bytes(view.status.storedBytes),
            })}
          {/if}
        </span>
      </span>
      <span class="ad-ctl">
        <button class="btn" disabled={busy} onclick={() => runCheck(false)}
          >{t('backups.check')}</button
        >
        <button class="btn" disabled={busy} onclick={list}>{t('backups.list')}</button>
        <button class="btn primary" disabled={busy || view.running} onclick={run}
          >{t('backups.now')}</button
        >
      </span>
    </li>
    {#if check}
      <li class={['ad-row', check.problems.length ? 'fail' : 'ok']} data-backups-check>
        <span class="ad-words">
          {#if check.problems.length === 0}
            <span
              >{t('backups.checkOk', {
                count: check.snapshots,
                size: bytes(check.storedBytes),
              })}</span
            >
          {:else}
            <span class="ad-bad">{t('backups.checkBad')}</span>
            {#each check.problems as p (p)}<span class="ad-help mono">{p}</span>{/each}
          {/if}
        </span>
        {#if check.problems.length === 0}
          <span class="ad-ctl">
            <button class="btn quiet" disabled={busy} onclick={() => runCheck(true)}
              >{t('backups.checkFull')}</button
            >
          </span>
        {/if}
      </li>
    {/if}
  </ul>

  {#if snapshots}
    <section class="ad-group" data-backups-list aria-labelledby="backups-kept">
      <h2 id="backups-kept">{t('backups.backups')}</h2>
      {#if snapshots.length === 0}
        <p class="ad-help">{t('backups.none')}</p>
      {:else}
        <ul class="ad-rows">
          {#each snapshots as s (s.id)}
            <li class="ad-row ad-tight" data-snapshot={s.id}>
              <span class="ad-words">
                <span class="ad-label">{dateTime(s.time)}</span>
                <span class="ad-help"
                  >{t('admin.backups.counts', {
                    users: s.counts.users ?? 0,
                    files: s.counts.files ?? 0,
                  })}, {bytes(s.size)}</span
                >
              </span>
              <span class="ad-ctl">
                <button class="btn quiet" disabled={busy} onclick={() => testRestore(s)}
                  >{t('backups.test')}</button
                >
              </span>
            </li>
          {/each}
        </ul>
        <h2>{t('admin.backups.person')}</h2>
        <ul class="ad-rows">
          <li class="ad-row">
            <span class="ad-words"
              ><span class="ad-help">{t('admin.backups.personHelp')}</span></span
            >
            <span class="ad-ctl">
              <input
                class="field person"
                aria-label={t('admin.backups.username')}
                placeholder={t('admin.backups.username')}
                bind:value={person}
              />
              <select
                class="field ad-select"
                aria-label={t('admin.backups.from')}
                bind:value={personSnapshot}
              >
                {#each snapshots as s (s.id)}<option value={s.id}>{dateTime(s.time)}</option>{/each}
              </select>
              <button
                class="btn"
                disabled={busy || !person.trim() || !personSnapshot}
                onclick={() => void restorePerson()}>{t('backups.restore')}</button
              >
            </span>
          </li>
        </ul>
      {/if}
    </section>
  {/if}

  <section class="ad-group" aria-labelledby="backups-when">
    <h2 id="backups-when">{t('backups.when')}</h2>
    <ul class="ad-rows">
      <li class="ad-row ad-tight">
        <span class="ad-words"><span class="ad-label">{t('backups.schedule')}</span></span>
        <span class="ad-ctl">
          <select class="field ad-select" aria-label={t('backups.schedule')} bind:value={schedule}>
            {#each SCHEDULES as s (s)}<option value={s}>{t(`backups.schedule_${s}`)}</option>{/each}
          </select>
        </span>
      </li>
      <li class="ad-row ad-tight">
        <span class="ad-words">
          <span class="ad-label">{t('admin.backups.keep')}</span>
          <span class="ad-help">{t('backups.keepHelp')}</span>
        </span>
        <span class="ad-ctl">
          {#each KEEP as period (period)}
            <label class="keep"
              ><span class="ad-unit">{t(`backups.keep_${period}`)}</span><input
                class="field ad-num"
                type="number"
                min="0"
                max="100"
                bind:value={policy[period]}
              /></label
            >
          {/each}
        </span>
      </li>
    </ul>
    <p class="actions">
      <button class="btn" disabled={busy} onclick={saveSchedule}>{t('backups.save')}</button>
    </p>
  </section>

  <section class="ad-group">
    <!-- rarely needed, long: folded -->
    <details class="recovery">
      <summary>{t('admin.backups.recovery')}</summary>
      <p class="ad-help">{t('admin.backups.recoveryHelp')}</p>
      <pre data-backups-recovery>{recovery}</pre>
    </details>
    <p class="actions">
      <button class="btn" disabled={busy} onclick={edit}>{t('backups.change')}</button>
      <button class="btn danger" disabled={busy} onclick={() => void stop()}
        >{t('backups.stop')}</button
      >
    </p>
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
      {#if view.suggestedApart}
        <p class="muted small">{t('admin.backups.simpleWhere')}</p>
      {:else}
        <p class="muted small">{t('admin.backups.simpleInData')}</p>
      {/if}
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
  form {
    display: grid;
    gap: var(--space-3);
    justify-items: start;
  }
  .person {
    width: 160px;
    height: 30px;
  }
  .keep {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .mono {
    font-family: var(--font-mono);
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .recovery summary {
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
  }
  .recovery[open] summary {
    margin-bottom: var(--space-2);
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
  .saved {
    color: var(--color-success);
  }
  .saved,
  .problem {
    margin-bottom: var(--space-4);
  }
</style>
