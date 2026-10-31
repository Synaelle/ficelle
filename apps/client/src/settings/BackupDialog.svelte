<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Make a backup of the whole vault, or put one back. A backup of an
     encrypted vault is encrypted too, with a password of its own; a plain
     vault's can be. Restoring shows what the backup holds and how much of
     it is here already before anything is written. -->
<script lang="ts">
  import { hour12 } from '../lib/clock.svelte'
  import PasswordField from '../lib/PasswordField.svelte'
  import { brand } from '@app/brand'
  import { onMount } from 'svelte'
  import { backend, type BackupManifest, type RestoreReport } from '../lib/backend'
  import { currentLocale, t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { modal } from '../lib/modal'
  import { pickToOpen, pickToSave } from '../lib/pick'
  import { isBackendError, workspace } from '../lib/workspace.svelte'

  let { mode, close }: { mode: 'make' | 'restore'; close: () => void } = $props()

  const MIN_PASSWORD = 8
  const filters = [{ name: brand.displayName, extensions: [brand.backupExtension] }]

  let vaultEncrypted = $state(false)
  let protect = $state(false)
  let password = $state('')
  let again = $state('')
  let busy = $state(false)
  let error = $state<string | null>(null)
  let made = $state<BackupManifest | null>(null)

  // restoring
  let path = $state<string | null>(null)
  let needsPassword = $state(false)
  let inspected = $state<{ manifest: BackupManifest; present: number } | null>(null)
  let restored = $state<RestoreReport | null>(null)

  const errorText = (err: unknown) => t(`errors.${isBackendError(err) ? err.kind : 'unknown'}`)
  const when = (time: number) =>
    new Intl.DateTimeFormat(currentLocale(), {
      dateStyle: 'medium',
      timeStyle: 'short',
      hour12: hour12(),
    }).format(time)

  onMount(async () => {
    vaultEncrypted = (await backend.vaultStatus()).encrypted
    protect = vaultEncrypted
    if (mode === 'restore') await chooseBackup()
  })

  const sealed = $derived(vaultEncrypted || protect)
  const passwordOk = $derived(!sealed || (password.length >= MIN_PASSWORD && password === again))

  async function make() {
    error = null
    const target = await pickToSave({
      title: t('backup.saveTitle'),
      defaultPath: await backend.backupDefaultName(),
      filters,
    })
    if (target === null) return
    busy = true
    try {
      made = await backend.backupWrite(target, sealed ? password : null)
    } catch (err) {
      error = errorText(err)
    } finally {
      busy = false
      password = ''
      again = ''
    }
  }

  async function chooseBackup() {
    error = null
    const picked = await pickToOpen({ title: t('backup.openTitle'), filters })
    if (picked === null) {
      if (!path) close()
      return
    }
    path = picked
    inspected = null
    password = ''
    try {
      needsPassword = await backend.backupEncrypted(picked)
      if (!needsPassword) await inspect()
    } catch (err) {
      error = errorText(err)
    }
  }

  async function inspect() {
    if (!path) return
    busy = true
    error = null
    try {
      inspected = await backend.backupInspect(path, needsPassword ? password : null)
    } catch (err) {
      error = errorText(err)
    } finally {
      busy = false
    }
  }

  async function restore() {
    if (!path) return
    busy = true
    error = null
    try {
      restored = await backend.backupRestore(path, needsPassword ? password : null)
      await Promise.all([workspace.refreshFolders(), workspace.refreshTagsAndNotes()])
    } catch (err) {
      error = errorText(err)
    } finally {
      busy = false
      password = ''
    }
  }

  const closeIfIdle = () => !busy && close()
  const toRestore = $derived(inspected ? inspected.manifest.notes - inspected.present : 0)
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
    aria-label={mode === 'make' ? t('backup.makeTitle') : t('backup.restoreTitle')}
    use:modal={{ onclose: closeIfIdle }}
  >
    <header>
      <h2>{mode === 'make' ? t('backup.makeTitle') : t('backup.restoreTitle')}</h2>
      <button class="icon-btn" aria-label={t('common.close')} disabled={busy} onclick={close}
        ><Icon name="close" /></button
      >
    </header>

    {#if mode === 'make' && !made}
      <p class="muted">{t('backup.makeHelp')}</p>
      {#if vaultEncrypted}
        <p>{t('backup.encryptedVault')}</p>
      {:else}
        <label class="check">
          <input type="checkbox" bind:checked={protect} />
          {t('backup.protect')}
        </label>
      {/if}
      {#if sealed}
        <PasswordField
          class="field"
          autocomplete="new-password"
          placeholder={t('backup.password')}
          aria-label={t('backup.password')}
          bind:value={password}
          data-autofocus
        />
        <PasswordField
          eye={false}
          class="field"
          autocomplete="new-password"
          placeholder={t('backup.passwordAgain')}
          aria-label={t('backup.passwordAgain')}
          bind:value={again}
        />
        <p class="muted small">{t('backup.passwordHelp', { count: MIN_PASSWORD })}</p>
      {/if}
    {:else if mode === 'make' && made}
      <p role="status">{t('backup.made', { count: made.notes, files: made.files })}</p>
      {#if made.filesMissing}
        <p class="muted">{t('backup.filesMissing', { count: made.filesMissing })}</p>
      {/if}
    {:else if restored}
      <p role="status">
        {t('backup.restored', { count: restored.notes, folders: restored.folders })}
      </p>
      {#if restored.skipped}
        <p class="muted">{t('backup.skipped', { count: restored.skipped })}</p>
      {/if}
      {#if restored.filesMissing}
        <p class="muted">{t('backup.filesMissingRestore', { count: restored.filesMissing })}</p>
      {/if}
    {:else if inspected}
      <dl class="counts">
        <dt>{t('backup.madeAt')}</dt>
        <dd>{when(inspected.manifest.createdAt)}</dd>
        <dt>{t('backup.by')}</dt>
        <dd>{inspected.manifest.app} {inspected.manifest.appVersion}</dd>
        <dt>{t('import.notes')}</dt>
        <dd>{inspected.manifest.notes}</dd>
        <dt>{t('import.folders')}</dt>
        <dd>{inspected.manifest.folders}</dd>
        <dt>{t('import.files')}</dt>
        <dd>{inspected.manifest.files}</dd>
      </dl>
      {#if inspected.present}
        <p class="muted">{t('backup.present', { count: inspected.present })}</p>
      {/if}
      {#if !toRestore}<p>{t('backup.nothing')}</p>{/if}
    {:else if path && needsPassword}
      <p>{t('backup.needsPassword')}</p>
      <form onsubmit={(e) => (e.preventDefault(), void inspect())}>
        <PasswordField
          class="field"
          autocomplete="current-password"
          placeholder={t('backup.password')}
          aria-label={t('backup.password')}
          bind:value={password}
          data-autofocus
        />
      </form>
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="actions">
      <span class="grow"></span>
      {#if mode === 'make' && !made}
        <button class="btn" disabled={busy} onclick={close}>{t('common.cancel')}</button>
        <button class="btn primary" disabled={busy || !passwordOk} onclick={make}
          ><Icon name="export" size={16} />{busy ? t('backup.making') : t('backup.save')}</button
        >
      {:else if mode === 'restore' && !restored && inspected && toRestore}
        <button class="btn" disabled={busy} onclick={chooseBackup}>{t('backup.another')}</button>
        <button class="btn primary" disabled={busy} onclick={restore}
          >{t('backup.restore', { count: toRestore })}</button
        >
      {:else if mode === 'restore' && !restored && !inspected && needsPassword}
        <button class="btn" disabled={busy} onclick={close}>{t('common.cancel')}</button>
        <button class="btn primary" disabled={busy || !password} onclick={inspect}
          >{t('backup.open')}</button
        >
      {:else}
        <button class="btn" disabled={busy} onclick={close}>{t('common.close')}</button>
      {/if}
    </div>
  </div>
</div>

<style>
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
    width: min(480px, 100%);
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
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
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
