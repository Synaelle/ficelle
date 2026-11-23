<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Data and backups, one page on desktop and phone: backups made by
     themselves as backup files in a folder, a backup file made by hand,
     restoring, moving notes in and out, starting over. The snapshot backups
     of earlier versions are only restored from, folded away at the end. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { MOBILE, WEB } from '../lib/platform'
  import BackupDialog from './BackupDialog.svelte'
  import { onMount } from 'svelte'
  import { backend } from '../lib/backend'
  import type { BackupFile, BackupSchedule } from '../lib/backend/types'
  import { backupError, backups } from '../lib/backups.svelte'
  import { errorLine, type ShownError } from '../lib/errors'
  import ErrorText from '../lib/ErrorText.svelte'
  import { bytes, dateTime, relative } from '../lib/format'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { MARKDOWN_ACCEPT } from '../lib/importing'
  import { overlay } from '../lib/overlay.svelte'
  import PasswordField from '../lib/PasswordField.svelte'
  import { pickToOpen } from '../lib/pick'
  import { workspace } from '../lib/workspace.svelte'
  import type { SettingsState } from './settingsState.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Switch from './ui/Switch.svelte'

  let {
    settings,
    onimport,
    onrestorefile,
    onreset,
  }: {
    settings: SettingsState
    /** `true` for this app's own export. */
    onimport: (ficelle: boolean) => void
    onrestorefile: () => void
    onreset: () => void
  } = $props()

  const view = $derived(backups.view)
  const config = $derived(view?.config ?? null)

  /** Choosing the folder to turn backups on in. */
  let setup = $state(false)
  /** A backup file being made by hand, see BackupDialog. */
  let making = $state(false)
  /** A phone names its own backup folder: there is nothing to pick there. */
  const phoneFolder = window.AppSystem?.backupFolder?.() ?? ''
  let folder = $state(phoneFolder)
  /** The folder is there, but the app may not read it. */
  let unreadable = $state(false)
  /** The notes are not encrypted: their backups need no password either. */
  let openDevice = $state(false)
  /** The notes are encrypted: an export is not, the export says so. */
  let encrypted = $state(false)
  let password = $state('')

  let files = $state<BackupFile[] | null>(null)
  /** A file made with another notes password than today's: its own asked. */
  let askingFor: string | null = $state(null)
  let filePassword = $state('')
  let busy = $state(false)
  let message: string | null = $state(null)
  let failure: ShownError | null = $state(null)
  let markdownInput: HTMLInputElement | undefined = $state()

  onMount(() => {
    void backend.vaultStatus().then((v) => {
      openDevice = !v.encrypted
      encrypted = v.encrypted
    })
    void backups.refresh()
  })

  async function act(action: () => Promise<void>, done?: string) {
    busy = true
    message = null
    failure = null
    try {
      await action()
      if (done) message = done
    } catch (err) {
      failure = backupError(err)
      if (failure.kind === 'unreadable') unreadable = true
    } finally {
      busy = false
    }
  }

  function openSetup(on: boolean) {
    setup = on
    password = ''
    unreadable = false
    message = null
    failure = null
  }

  /** A phone's folder an earlier install made: the system's picker lets
   * this one in. */
  function allow() {
    void act(async () => {
      const picked = await backend.backupsPickFolder()
      if (picked) {
        folder = picked
        unreadable = false
      }
    })
  }

  async function chooseFolder() {
    const picked = await pickToOpen({ directory: true, title: t('backups.chooseFolder') })
    if (picked) folder = picked
  }

  function turnOn() {
    void act(async () => {
      backups.view = await backend.backupsTurnOn(folder.trim(), openDevice ? null : password)
      password = ''
      setup = false
      // the first backup at once, so the person sees it work
      await backups.run()
    }, t('backups.connected'))
  }

  async function stop() {
    const yes = await overlay.ask(t('backups.stopConfirm'), {
      confirmLabel: t('backups.stop'),
      danger: true,
    })
    if (!yes) return
    void act(async () => {
      backups.view = await backend.backupsTurnOff()
      files = null
    })
  }

  function change(schedule: BackupSchedule, keep: number) {
    void act(async () => {
      backups.view = await backend.backupsSettings(schedule, keep)
    })
  }

  function runNow() {
    void act(async () => {
      await backups.run()
      if (files) files = await backend.backupsList()
    }, t('backups.done'))
  }

  function list() {
    if (files) return void (files = null)
    void act(async () => {
      files = await backend.backupsList()
    })
  }

  /** When an automatic backup was made, from its name, in local time. */
  function madeAt(name: string): number | null {
    const m = /(\d{4})-(\d{2})-(\d{2})-(\d{2})(\d{2})(\d{2})\.\w+$/.exec(name)
    if (!m) return null
    const [, y, mo, d, h, mi, se] = m.map(Number) as number[]
    return new Date(y ?? 0, (mo ?? 1) - 1, d ?? 1, h ?? 0, mi ?? 0, se ?? 0).getTime()
  }

  const label = (file: BackupFile) => {
    const at = file.automatic ? madeAt(file.name) : null
    return at === null ? file.name : dateTime(at)
  }

  function restored(report: { notes: number; skipped: number }) {
    message = t('backup.restored', { count: report.notes })
    if (report.skipped) message += ` ${t('backup.skipped', { count: report.skipped })}`
  }

  async function restore(file: BackupFile) {
    const yes = await overlay.ask(t('backups.restoreConfirm', { when: label(file) }), {
      confirmLabel: t('backups.restore'),
    })
    if (!yes) return
    void act(async () => {
      try {
        const report = await backend.backupsRestore(file.name, null)
        await workspace.load()
        restored(report)
      } catch (err) {
        // made with an earlier notes password: that one is asked
        if ((err as { kind?: string })?.kind !== 'wrongPassword') throw err
        askingFor = file.name
        filePassword = ''
      }
    })
  }

  function restoreWithPassword() {
    const name = askingFor
    if (!name) return
    void act(async () => {
      const report = await backend.backupsRestore(name, filePassword)
      askingFor = null
      filePassword = ''
      await workspace.load()
      restored(report)
    })
  }

  /** A folder picked on a phone is an address: its path as the Files app
   * shows it (`primary:Documents/Ficelle backups` at the end, encoded). */
  function shown(path: string): string {
    if (!path.startsWith('content://')) return path
    const id = decodeURIComponent(path.slice(path.lastIndexOf('/') + 1))
    return id.slice(id.indexOf(':') + 1)
  }

  /** Where the files go: a folder of their own inside the chosen one, as
   * the app's backups command places them (`app_core::repo::folder_name`),
   * unless the chosen one is that folder already. A phone's folder takes
   * them as it is. */
  function filesFolder(chosen: string): string {
    if (chosen.startsWith('content://')) return chosen
    const trimmed = chosen.replace(/[\\/]+$/, '')
    const last = trimmed.slice(Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\')) + 1)
    if (last.startsWith(brand.displayName) && last.endsWith(' backups')) return trimmed
    const separator = trimmed.includes('\\') && !trimmed.includes('/') ? '\\' : '/'
    return `${trimmed}${separator}${brand.displayName} backups`
  }

  /** What is true of the backups now, in a few words. */
  const health = $derived.by(() => {
    const status = view?.status
    if (!status) return ''
    if (view?.running) return t('backups.running')
    if (status.lastError) return t('backups.failedTitle')
    if (status.lastSuccessAt) return t('backups.backedUp', { when: relative(status.lastSuccessAt) })
    return t('backups.never')
  })

  const SCHEDULES: BackupSchedule[] = ['hourly', 'daily', 'weekly', 'off']

  async function importPicked(input: HTMLInputElement) {
    await workspace.importFiles([...(input.files ?? [])])
    input.value = ''
  }
</script>

{#snippet chevron(open: boolean)}
  <span class="chevron" class:open><Icon name="down" size={16} /></span>
{/snippet}

{#if !backend.native && !WEB}<p class="settings-help">{t('errors.unsupported')}</p>{/if}

{#if WEB}
  <!-- the browser keeps the notes and can lose them: copies are the
       person's to make, there is no folder to write to by itself -->
  <Group title={t('backups.title')} note={t('backups.webHelp')}>
    <Row label={t('backups.download')} icon="export" opens onclick={() => (making = true)} />
    <Row label={t('backups.restoreFile')} icon="folderOpen" opens onclick={onrestorefile} />
  </Group>
{:else}
  <Group title={t('backups.title')}>
    <Row label={t('backups.auto')} icon="backup">
      <Switch
        label={t('backups.auto')}
        checked={!!config}
        disabled={busy}
        onchange={(on) => {
          if (on) openSetup(true)
          else if (config) void stop()
          else openSetup(false)
        }}
      />
    </Row>
    {#if config}
      <Row
        label={health}
        icon="info"
        help={view?.status.lastError && !view.running
          ? errorLine(
              backupError({
                kind: view.status.lastErrorKind ?? 'unknown',
                message: view.status.lastError,
              }),
            )
          : view?.status.lastSuccessAt
            ? t('backups.size', { count: view.status.files, size: bytes(view.status.storedBytes) })
            : undefined}
      >
        <button class="btn" disabled={busy || view?.running} onclick={runNow}
          >{t('backups.now')}</button
        >
      </Row>
      <Row label={t('backups.place')} icon="folder" help={shown(filesFolder(config.folder))} />
      <Row label={t('backups.when')} icon="timer">
        <select
          class="field compact same-width"
          aria-label={t('backups.when')}
          value={config.schedule}
          disabled={busy}
          onchange={(e) => change(e.currentTarget.value as BackupSchedule, config.keep)}
        >
          {#each SCHEDULES as schedule (schedule)}
            <option value={schedule}>{t(`backups.schedule_${schedule}`)}</option>
          {/each}
        </select>
      </Row>
      <Row label={t('backups.kept')} icon="allNotes">
        <input
          class="field compact number"
          type="number"
          min="1"
          max="100"
          aria-label={t('backups.kept')}
          value={config.keep}
          disabled={busy}
          onchange={(e) => {
            const keep = Math.round(Number(e.currentTarget.value))
            if (keep >= 1 && keep <= 100) change(config.schedule, keep)
          }}
        />
      </Row>
    {/if}
    <Row label={t('backup.makeTitle')} icon="export" opens onclick={() => (making = true)} />
  </Group>

  {#if setup}
    <div class="fold-body">
      <Group>
        <div class="inside">
          {#if phoneFolder}
            <p class="settings-help">{t('backups.phoneFolder')}</p>
            <code>{shown(folder)}</code>
          {:else}
            <label class="settings-field">
              {t('backups.folder')}
              <span class="line">
                <input class="field" autocomplete="off" bind:value={folder} />
                <button type="button" class="btn" onclick={() => void chooseFolder()}
                  >{t('backups.chooseFolder')}</button
                >
              </span>
            </label>
          {/if}
          {#if unreadable}
            <p class="settings-help bad" role="status">
              {MOBILE
                ? t('backups.unreadablePhone', { app: brand.displayName })
                : t('backups.unreadable', { app: brand.displayName })}
            </p>
          {/if}
          {#if !openDevice}
            <label class="settings-field">
              {t('backups.notesPassword')}
              <PasswordField class="field" autocomplete="current-password" bind:value={password} />
            </label>
          {/if}
          <div class="settings-actions">
            <button
              class="btn primary"
              disabled={busy || !folder.trim() || (!openDevice && !password)}
              onclick={turnOn}>{t('backups.turnOn')}</button
            >
            {#if unreadable && MOBILE}
              <button class="btn" type="button" disabled={busy} onclick={allow}
                >{t('backups.allowFolder')}</button
              >
            {/if}
            <button class="btn" type="button" onclick={() => openSetup(false)}
              >{t('common.cancel')}</button
            >
          </div>
        </div>
      </Group>
    </div>
  {/if}

  {#if failure}<p class="settings-help bad" role="alert"><ErrorText error={failure} /></p>{/if}
  {#if message}<p class="settings-help" role="status">{message}</p>{/if}

  <Group title={t('backups.restoreTitle')}>
    {#if config}
      <Row label={t('backups.restoreFrom')} icon="restore" onclick={list}>
        {@render chevron(!!files)}
      </Row>
      {#if files}
        <div class="fold-rows">
          {#if files.length === 0}
            <p class="settings-help inside">{t('backups.none')}</p>
          {/if}
          {#each files as file (file.name)}
            <Row label={label(file)} help={bytes(file.size)}>
              <button class="btn" disabled={busy} onclick={() => void restore(file)}
                >{t('backups.restore')}</button
              >
            </Row>
            {#if askingFor === file.name}
              <div class="inside">
                <label class="settings-field">
                  {t('backups.password')}
                  <PasswordField
                    class="field"
                    autocomplete="current-password"
                    bind:value={filePassword}
                  />
                </label>
                <div class="settings-actions">
                  <button
                    class="btn primary"
                    disabled={busy || !filePassword}
                    onclick={restoreWithPassword}>{t('backups.restore')}</button
                  >
                  <button class="btn" type="button" onclick={() => (askingFor = null)}
                    >{t('common.cancel')}</button
                  >
                </div>
              </div>
            {/if}
          {/each}
        </div>
      {/if}
    {/if}
    <Row
      label={t('backups.restoreFile')}
      icon="folderOpen"
      opens
      disabled={!backend.native}
      onclick={onrestorefile}
    />
  </Group>
{/if}
{#if making}<BackupDialog mode="make" close={() => (making = false)} />{/if}

<Group title={t('settings.importTitle')}>
  <Row label={t('list.import')} icon="markdown" opens onclick={() => markdownInput?.click()} />
  <Row
    label={t('settings.importFicelle', { app: brand.displayName })}
    icon="import"
    opens
    disabled={!backend.native && !WEB}
    onclick={() => onimport(true)}
  />
  <Row
    label={t('settings.import')}
    icon="import"
    opens
    disabled={!backend.native && !WEB}
    onclick={() => onimport(false)}
  />
</Group>
<input
  bind:this={markdownInput}
  type="file"
  accept={MARKDOWN_ACCEPT}
  multiple
  hidden
  onchange={(e) => importPicked(e.currentTarget)}
/>

<Group title={t('settings.exportTitle')} note={encrypted ? t('settings.exportPlain') : undefined}>
  <Row
    label={t('settings.exportZip')}
    icon="archive"
    opens
    disabled={!backend.native && !WEB}
    onclick={() => settings.exportZip()}
  />
</Group>

<Group>
  <Row label={t('reset.title')} icon="trash">
    <button class="btn danger" onclick={onreset}>{t('reset.start')}</button>
  </Row>
</Group>

<style>
  .inside {
    display: grid;
    gap: var(--space-3);
    padding: var(--space-3);
  }
  p.inside {
    margin: 0;
  }
  p {
    margin: 0;
  }
  .line {
    display: flex;
    gap: var(--space-2);
  }
  .line .field {
    flex: 1;
    min-width: 0;
  }
  code {
    font-family: var(--font-ui);
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .bad {
    color: var(--color-danger);
  }
  .settings-actions {
    justify-content: flex-end;
  }
  .chevron {
    display: inline-block;
    color: var(--color-text-faint);
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .chevron.open {
    transform: rotate(180deg);
  }
  /* as the language and time format boxes */
  .field.same-width {
    width: 15rem;
    max-width: 100%;
  }
  .field.compact.number {
    width: 5rem;
    min-width: 5rem;
  }
</style>
