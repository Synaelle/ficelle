<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Data and backups, one page on desktop and phone: backups running by
     themselves, restoring, moving notes in and out, starting over. Backups
     made earlier, and ones set up with S3 or WebDAV, keep working: they show
     where they go and can be stopped. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { MOBILE } from '../lib/platform'
  import { onMount } from 'svelte'
  import { backend } from '../lib/backend'
  import type { BackupSnapshot, BackupTarget } from '../lib/backend/types'
  import { backupError, backups } from '../lib/backups.svelte'
  import { bytes, dateTime, relative } from '../lib/format'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import PasswordField from '../lib/PasswordField.svelte'
  import { MIN_PASSWORD } from '../lib/passwords'
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

  /** Choosing a folder: to back up to it, or to restore from what it holds. */
  let setup: 'start' | 'restore' | null = $state(null)
  /** A phone names its own backup folder: there is nothing to pick there. */
  const phoneFolder = window.AppSystem?.backupFolder?.() ?? ''
  let folder = $state(phoneFolder)
  /** The folder looked into, null while looking or before. */
  let looked: string | null = $state(null)
  let looking = $state(false)
  let found = $state(false)
  /** The folder is there, but the app may not read it. */
  let unreadable = $state(false)
  let needsPassword = $state(true)
  /** The notes are not encrypted: their backups need no password either. */
  let openDevice = $state(false)
  let sameAsNotes = $state(true)
  let password = $state('')
  let again = $state('')

  let snapshots = $state<BackupSnapshot[] | null>(null)
  let busy = $state(false)
  let message: string | null = $state(null)
  let failure: string | null = $state(null)
  let markdownInput: HTMLInputElement | undefined = $state()

  onMount(() => {
    void backend.vaultStatus().then((v) => (openDevice = !v.encrypted))
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
      const e = err as { kind?: string; message?: string }
      failure = backupError(e?.kind, e?.message ?? String(err))
    } finally {
      busy = false
    }
  }

  function open(next: 'start' | 'restore' | null) {
    setup = next
    password = ''
    again = ''
    message = null
    failure = null
    if (next) void look()
  }

  async function look() {
    const path = folder.trim()
    looked = null
    if (!path) return
    looking = true
    unreadable = false
    try {
      const there = await backend.backupsFound({ kind: 'folder', path })
      found = there.found
      needsPassword = there.needsPassword
    } catch (err) {
      found = false
      needsPassword = true
      unreadable = (err as { kind?: string })?.kind === 'unreadable'
    } finally {
      looking = false
      looked = path
    }
  }

  async function chooseFolder() {
    const picked = await pickToOpen({ directory: true, title: t('backups.chooseFolder') })
    if (picked) {
      folder = picked
      await look()
    }
  }

  /** What stops the button, for starting in a new folder. */
  const newPasswordProblem = $derived.by(() => {
    if (openDevice) return null
    if (!password) return t('backups.passwordNeeded')
    if (sameAsNotes) return null
    if (password.length < MIN_PASSWORD) return t('backups.passwordShort')
    return password === again ? null : t('backups.passwordsDiffer')
  })

  const target = (): BackupTarget => ({ kind: 'folder', path: folder.trim() })

  /** Backups already in the folder: opened, and no new one over them yet, it
   * would be the newest and hold only what is here now. */
  function openFound() {
    const restoring = setup === 'restore'
    void act(async () => {
      // backups made without a password take the notes' one
      if (!needsPassword && !openDevice && !(await backend.vaultCheckPassword(password)))
        throw new Error(t('backups.notesPasswordWrong'))
      backups.view = await backend.backupsConnect(target(), password, false, '')
      password = ''
      setup = null
      snapshots = await backend.backupsSnapshots()
      if (restoring && snapshots.length > 0) await offerLatest()
    }, t('backups.foundOpened'))
  }

  function startNew() {
    void act(async () => {
      if (!openDevice && sameAsNotes) {
        const ok = await backend.vaultCheckPassword(password)
        if (!ok) throw new Error(t('backups.notesPasswordWrong'))
      }
      backups.view = await backend.backupsConnect(target(), openDevice ? '' : password, true, '')
      password = ''
      again = ''
      setup = null
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
      backups.view = await backend.backupsDisconnect()
      snapshots = null
    })
  }

  function runNow() {
    void act(async () => {
      await backups.run()
      if (snapshots) snapshots = await backend.backupsSnapshots()
    }, t('backups.done'))
  }

  function list() {
    if (snapshots) return void (snapshots = null)
    void act(async () => {
      snapshots = await backend.backupsSnapshots()
    })
  }

  const newestFirst = $derived([...(snapshots ?? [])].sort((a, b) => b.time - a.time))

  function restoreNow(snapshot: BackupSnapshot) {
    void act(async () => {
      const report = await backend.backupsRestore(snapshot.id)
      await workspace.load()
      message = t('backups.restored', { notes: report.notes, skipped: report.skipped })
    })
  }

  async function restore(snapshot: BackupSnapshot) {
    const yes = await overlay.ask(t('backups.restoreConfirm', { when: dateTime(snapshot.time) }), {
      confirmLabel: t('backups.restore'),
    })
    if (yes) restoreNow(snapshot)
  }

  /** Right after backups from before are opened: the newest is likely the one
   * wanted, the list stays for another. */
  async function offerLatest() {
    const latest = newestFirst[0]
    if (!latest) return
    const yes = await overlay.ask(
      t('backups.latestAsk', {
        when: dateTime(latest.time),
        notes: t('backups.notes', { count: latest.counts.notes ?? 0 }),
      }),
      { confirmLabel: t('backups.latestRestore') },
    )
    if (yes) restoreNow(latest)
  }

  function where(): string {
    const place = config?.target
    if (!place) return ''
    if (place.kind === 'folder') return place.path
    if (place.kind === 's3') {
      const host = place.endpoint
        ? place.endpoint.replace(/^https?:\/\//, '').replace(/\/$/, '')
        : `AWS ${place.region}`
      return `${place.bucket}${place.prefix ? `/${place.prefix}` : ''}, ${host}`
    }
    return place.url
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

  async function importPicked(input: HTMLInputElement) {
    await workspace.importFiles([...(input.files ?? [])])
    input.value = ''
  }
</script>

{#snippet chevron(open: boolean)}
  <span class="chevron" class:open><Icon name="down" size={16} /></span>
{/snippet}

{#if !backend.native}<p class="settings-help">{t('errors.unsupported')}</p>{/if}

<Group title={t('backups.title')} note={phoneFolder ? t('backups.phoneHelp') : t('backups.help')}>
  <Row label={t('backups.auto')}>
    <Switch
      label={t('backups.auto')}
      checked={!!config || setup === 'start'}
      disabled={busy}
      onchange={(on) => {
        if (on) open('start')
        else if (config) void stop()
        else open(null)
      }}
    />
  </Row>
  {#if config}
    <Row
      label={health}
      help={view?.status.lastError && !view.running
        ? backupError(view.status.lastErrorKind, view.status.lastError)
        : view?.status.lastSuccessAt
          ? t('backups.size', {
              count: view.status.snapshots,
              size: bytes(view.status.storedBytes),
            })
          : undefined}
    >
      <button class="btn" disabled={busy || view?.running} onclick={runNow}
        >{t('backups.now')}</button
      >
    </Row>
    <Row label={t('backups.place')} help={where()} />
  {/if}
</Group>

{#if setup}
  <div class="fold-body">
    <Group title={setup === 'restore' ? t('backups.restoreFrom') : undefined}>
      <div class="inside">
        {#if phoneFolder}
          <p class="settings-help">
            {setup === 'restore' ? t('backups.phoneFolderRestore') : t('backups.phoneFolder')}
          </p>
          <code>{phoneFolder}</code>
        {:else}
          <label class="settings-field">
            {t('backups.folder')}
            <span class="line">
              <input
                class="field"
                autocomplete="off"
                bind:value={folder}
                onchange={() => void look()}
              />
              <button type="button" class="btn" onclick={() => void chooseFolder()}
                >{t('backups.chooseFolder')}</button
              >
            </span>
          </label>
        {/if}

        {#if looking}
          <p class="settings-help" role="status">{t('backups.looking')}</p>
        {:else if looked !== null && looked === folder.trim()}
          {#if unreadable}
            <p class="settings-help bad" role="status">
              {t(MOBILE ? 'backups.unreadablePhone' : 'backups.unreadable', {
                app: brand.displayName,
              })}
            </p>
            <div class="settings-actions">
              <button class="btn" type="button" onclick={() => open(null)}
                >{t('common.cancel')}</button
              >
            </div>
          {:else if found}
            <p class="settings-help" role="status">
              {setup === 'restore' ? t('backups.foundTitle') : t('backups.foundStartHelp')}
            </p>
            {#if needsPassword}
              <label class="settings-field">
                {t('backups.password')}
                <PasswordField
                  class="field"
                  autocomplete="current-password"
                  bind:value={password}
                />
              </label>
            {:else if !openDevice}
              <label class="settings-field">
                {t('backups.notesPassword')}
                <PasswordField
                  class="field"
                  autocomplete="current-password"
                  bind:value={password}
                />
              </label>
              <p class="settings-help">{t('backups.foundOpenHelp')}</p>
            {/if}
            <div class="settings-actions">
              <button class="btn" type="button" onclick={() => open(null)}
                >{t('common.cancel')}</button
              >
              <button
                class="btn primary"
                disabled={busy || ((needsPassword || !openDevice) && !password)}
                onclick={openFound}>{busy ? t('backups.connecting') : t('backups.useThem')}</button
              >
            </div>
          {:else if setup === 'restore'}
            <p class="settings-help bad" role="status">{t('backups.noneHere')}</p>
            <div class="settings-actions">
              <button class="btn" type="button" onclick={() => open(null)}
                >{t('common.cancel')}</button
              >
            </div>
          {:else}
            {#if !openDevice}
              <label class="settings-check">
                <input type="checkbox" bind:checked={sameAsNotes} />
                {t('backups.sameAsNotes')}
              </label>
              <label class="settings-field">
                {sameAsNotes ? t('backups.notesPassword') : t('backups.newPassword')}
                <PasswordField
                  class="field"
                  autocomplete={sameAsNotes ? 'current-password' : 'new-password'}
                  bind:value={password}
                />
              </label>
              {#if !sameAsNotes}
                <label class="settings-field">
                  {t('backups.passwordAgain')}
                  <PasswordField
                    eye={false}
                    class="field"
                    autocomplete="new-password"
                    bind:value={again}
                  />
                </label>
              {/if}
              <p class="settings-help">
                {sameAsNotes ? t('backups.sameAsNotesHelp') : t('backups.passwordWarning')}
              </p>
            {/if}
            <div class="settings-actions">
              <button class="btn" type="button" onclick={() => open(null)}
                >{t('common.cancel')}</button
              >
              <button class="btn primary" disabled={busy || !!newPasswordProblem} onclick={startNew}
                >{busy ? t('backups.connecting') : t('backups.turnOn')}</button
              >
            </div>
          {/if}
        {/if}
      </div>
    </Group>
  </div>
{/if}

{#if failure}<p class="settings-help bad" role="alert">{failure}</p>{/if}
{#if message}<p class="settings-help" role="status">{message}</p>{/if}

<Group title={t('backups.restoreTitle')}>
  {#if config}
    <Row label={t('backups.restoreFrom')} onclick={list}>
      {@render chevron(!!snapshots)}
    </Row>
    {#if snapshots}
      <div class="fold-rows">
        {#if newestFirst.length === 0}
          <p class="settings-help inside">{t('backups.none')}</p>
        {/if}
        {#each newestFirst as snapshot (snapshot.id)}
          <Row
            label={dateTime(snapshot.time)}
            help={`${t('backups.notes', { count: snapshot.counts.notes ?? 0 })}, ${t('backups.files', { count: snapshot.counts.files ?? 0 })}`}
          >
            <button class="btn" disabled={busy} onclick={() => void restore(snapshot)}
              >{t('backups.restore')}</button
            >
          </Row>
        {/each}
      </div>
    {/if}
  {:else}
    <Row label={t('backups.restoreFrom')} opens onclick={() => open('restore')} />
  {/if}
  <Row label={t('backups.restoreFile')} opens disabled={!backend.native} onclick={onrestorefile} />
</Group>

<Group title={t('settings.importTitle')} note={t('settings.importHelp')}>
  <Row label={t('list.import')} opens onclick={() => markdownInput?.click()} />
  <Row
    label={t('settings.importFicelle', { app: brand.displayName })}
    opens
    disabled={!backend.native}
    onclick={() => onimport(true)}
  />
  <Row
    label={t('settings.import')}
    opens
    disabled={!backend.native}
    onclick={() => onimport(false)}
  />
</Group>
<input
  bind:this={markdownInput}
  type="file"
  accept=".md,.markdown,.mdown,.mkd,text/markdown"
  multiple
  hidden
  onchange={(e) => importPicked(e.currentTarget)}
/>

<Group title={t('settings.exportTitle')} note={t('settings.exportHelp')}>
  <Row
    label={t('settings.exportZip')}
    opens
    disabled={!backend.native}
    onclick={() => settings.exportZip()}
  />
</Group>

<Group>
  <Row label={t('reset.title')} help={t('reset.help')}>
    <button class="btn danger" onclick={onreset}>{t('reset.open')}</button>
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
    font-family: var(--font-mono);
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
</style>
