<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The instance's backups: the folder on the server, the password, how
     many to keep, made daily; how the runs went; the backups to check and
     test restore. A whole instance is restored from the command line, the
     server stopped: what to keep for that is shown here. Setting up starts
     simple, a new backup in the suggested folder and a password; Advanced
     also opens a backup already there. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { t } from '../lib/i18n.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api } from './api.svelte'
  import { bytes, dateTime, errorText, relative } from './format'

  type Target = { kind: 'folder'; path: string }
  interface View {
    config: { target: Target; keep: number } | null
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

  /** `onconfigured` hears whether backups are set up, each time it is known. */
  let { onconfigured }: { onconfigured?: (on: boolean) => void } = $props()

  let view: View | null = $state(null)
  /** The choice of a backup already there, rather than a new one. */
  let advanced = $state(false)
  let problem = $state('')
  let saved = $state('')
  let busy = $state(false)
  let editing = $state(false)
  let path: string | null = $state(null)
  let existing = $state(false)
  let password = $state('')
  let again = $state('')
  let keep = $state(30)
  let snapshots: Snapshot[] | null = $state(null)
  let check: Check | null = $state(null)

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
    if (next.config) keep = next.config.keep
  }

  void act(async () => show(await api<View>('GET', '/admin/backups')))

  function edit() {
    const target = view?.config?.target
    path = target?.path ?? ''
    existing = !!target
    password = ''
    again = ''
    advanced = true
    editing = true
  }

  /** The simple setup: a new repository in the suggested folder. */
  function simple() {
    advanced = false
    path = view?.suggestedFolder ?? ''
    existing = false
  }

  $effect(() => {
    if (view && !view.config && !advanced && path === null) simple()
  })

  const passwordProblem = $derived.by(() => {
    if (!password) return t('backups.passwordNeeded')
    if (!existing && password.length < 8) return t('backups.passwordShort')
    if (!existing && password !== again) return t('backups.passwordsDiffer')
    return null
  })

  function connect() {
    void act(async () => {
      show(
        await api<View>('PUT', '/admin/backups', {
          target: { kind: 'folder', path: path ?? '' },
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

  function saveKeep(value: number) {
    const count = Math.round(value)
    if (count < 1 || count > 100) return
    void act(async () => {
      show(
        await api<View>('PUT', '/admin/backups/settings', {
          keep: count,
          offsetSeconds: offsetSeconds(),
        }),
      )
    }, t('backups.saved'))
  }

  function list() {
    void act(async () => {
      snapshots = await api<Snapshot[]>('GET', '/admin/backups/snapshots')
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
  const where = $derived((view as View | null)?.config?.target.path ?? '')
</script>

{#if problem}<p class="problem" role="alert" data-backups-problem>{problem}</p>{/if}
{#if saved}<p class="saved" role="status" data-backups-saved>{saved}</p>{/if}

<!-- the simple setup says what it needs in two lines, the rest comes with
     Advanced or once backups run -->
{#if view && (view.config || advanced)}
  <section>
    {#if !view.masterKeyApart}
      <p class="muted small">{t('admin.backups.masterKey')}</p>
    {:else}
      <p class="note">{t('admin.backups.masterKeyApart')}</p>
    {/if}
  </section>
{/if}

{#if view?.config && !editing}
  <ul class="dash-rows" data-backups-status>
    <li
      class={[
        'dash-row',
        view.status.lastError ? 'fail' : view.status.lastSuccessAt ? 'ok' : 'warn',
      ]}
    >
      <span class="dash-lead"
        ><Icon
          name={view.status.lastError || !view.status.lastSuccessAt ? 'warning' : 'check'}
          size={18}
        /></span
      >
      <span class="dash-words">
        <span class="dash-label">
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
          <span class="dash-help dash-bad"
            >{t('backups.lastFailed', { error: view.status.lastError })}</span
          >
        {/if}
        <span class="dash-help">
          {#if where}{t('backups.where', { where })}{/if}
          {#if view.status.snapshots > 0}
            {t('backups.size', {
              count: view.status.snapshots,
              size: bytes(view.status.storedBytes),
            })}
          {/if}
        </span>
      </span>
      <span class="dash-ctl">
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
      <li class={['dash-row', check.problems.length ? 'fail' : 'ok']} data-backups-check>
        <span class="dash-words">
          {#if check.problems.length === 0}
            <span
              >{t('backups.checkOk', {
                count: check.snapshots,
                size: bytes(check.storedBytes),
              })}</span
            >
          {:else}
            <span class="dash-bad">{t('backups.checkBad')}</span>
            {#each check.problems as p (p)}<span class="dash-help mono">{p}</span>{/each}
          {/if}
        </span>
        {#if check.problems.length === 0}
          <span class="dash-ctl">
            <button class="btn quiet" disabled={busy} onclick={() => runCheck(true)}
              >{t('backups.checkFull')}</button
            >
          </span>
        {/if}
      </li>
    {/if}
  </ul>

  {#if snapshots}
    <section class="dash-group" data-backups-list aria-labelledby="backups-kept">
      <h2 id="backups-kept">{t('backups.backups')}</h2>
      {#if snapshots.length === 0}
        <p class="dash-help">{t('backups.none')}</p>
      {:else}
        <ul class="dash-rows">
          {#each snapshots as s (s.id)}
            <li class="dash-row dash-tight" data-snapshot={s.id}>
              <span class="dash-words">
                <span class="dash-label">{dateTime(s.time)}</span>
                <span class="dash-help"
                  >{t('admin.backups.counts', {
                    users: s.counts.users ?? 0,
                    files: s.counts.files ?? 0,
                  })}, {bytes(s.size)}</span
                >
              </span>
              <span class="dash-ctl">
                <button class="btn quiet" disabled={busy} onclick={() => testRestore(s)}
                  >{t('backups.test')}</button
                >
              </span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}

  <ul class="dash-rows">
    <li class="dash-row dash-tight">
      <span class="dash-words"><span class="dash-label">{t('backups.kept')}</span></span>
      <span class="dash-ctl">
        <input
          class="field dash-num"
          type="number"
          min="1"
          max="100"
          aria-label={t('backups.kept')}
          value={keep}
          disabled={busy}
          onchange={(e) => saveKeep(Number(e.currentTarget.value))}
        />
      </span>
    </li>
  </ul>

  <section class="dash-group">
    <!-- rarely needed, long: folded -->
    <details class="recovery">
      <summary>{t('admin.backups.recovery')}</summary>
      <p class="dash-help">{t('admin.backups.recoveryHelp')}</p>
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
    <label class="field-label"
      >{t('admin.backups.field_path')}<input
        class="field wide"
        value={path ?? ''}
        oninput={(e) => (path = e.currentTarget.value)}
        autocomplete="off"
      /></label
    >
    {#if !advanced}
      {#if view.suggestedApart}
        <p class="muted small">{t('admin.backups.simpleWhere')}</p>
      {:else}
        <p class="muted small">{t('admin.backups.simpleInData')}</p>
      {/if}
    {:else}
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
  .mono {
    font-family: var(--font-ui);
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
