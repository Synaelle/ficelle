// SPDX-License-Identifier: AGPL-3.0-or-later
// What the desktop dialog and the phone settings share: the vault status and
// the actions that change it. Each view only lays them out its own way.

import { backend, PIN_LENGTH, type VaultStatus, type VaultWay } from '../lib/backend'
import { t } from '../lib/i18n.svelte'
import { pickToOpen, pickToSave } from '../lib/pick'
import { overlay } from '../lib/overlay.svelte'
import { isBackendError } from '../lib/workspace.svelte'

/** Choices for the auto lock delay in minutes, 0 is never. */
export const LOCK_CHOICES = [0, 1, 5, 15, 30, 60]

export function lockLabel(minutes: number): string {
  return minutes === 0
    ? t('settings.autoLockNever')
    : t('settings.autoLockMinutes', { count: minutes })
}

/** The ways this device can open the vault day to day, in the order they
 * are shown. The password always opens it as well. */
export function ways(vault: VaultStatus): VaultWay[] {
  const out: VaultWay[] = []
  if (vault.biometricAvailable || vault.way === 'guarded') out.push('guarded')
  if (vault.deviceAvailable || vault.way === 'pin') out.push('pin')
  out.push('password')
  return out
}

const MIN_PASSWORD = 8

const errorText = (err: unknown) => t(`errors.${isBackendError(err) ? err.kind : 'unknown'}`)

export class SettingsState {
  vault: VaultStatus | null = $state(null)
  /** Last outcome to show, success or error, already translated. */
  message = $state('')
  /** A new recovery key to show once, empty otherwise. */
  recoveryCode = $state('')

  async refresh() {
    this.vault = await backend.vaultStatus()
  }

  /** What proves the right to change the vault: its password, or nothing
   * when Windows Hello is the way (it asks) or an old vault has none.
   * `undefined` when the person cancelled. */
  private async currentPassword(): Promise<string | null | undefined> {
    if (!this.vault?.hasPassword || this.vault.way === 'guarded') return null
    return (await overlay.askPassword(t('settings.currentPasswordPrompt'))) || undefined
  }

  /** A new password, typed twice. `null` when cancelled or refused, the
   * reason in `message`. */
  private async newPassword(): Promise<string | null> {
    const password = await overlay.askPassword(t('settings.newPassword'))
    if (!password) return null
    if (password.length < MIN_PASSWORD) {
      this.message = t('setup.tooShort', { count: MIN_PASSWORD })
      return null
    }
    if ((await overlay.askPassword(t('setup.confirm'))) !== password) {
      this.message = t('setup.mismatch')
      return null
    }
    return password
  }

  /** A new PIN, typed twice. */
  private async newPin(): Promise<string | null> {
    const pin = await overlay.askPin(t('pin.choose', { count: PIN_LENGTH }))
    if (!pin) return null
    if (pin.length !== PIN_LENGTH || !/^\d+$/.test(pin)) {
      this.message = t('pin.digits', { count: PIN_LENGTH })
      return null
    }
    if ((await overlay.askPin(t('pin.confirm'))) !== pin) {
      this.message = t('pin.mismatch')
      return null
    }
    return pin
  }

  /** Changes how the vault opens day to day. An old vault without a
   * password is given one first. */
  async setWay(way: VaultWay) {
    this.message = ''
    let newPassword: string | null = null
    if (!this.vault?.hasPassword) {
      newPassword = await this.newPassword()
      if (!newPassword) return
    }
    let pin: string | null = null
    if (way === 'pin') {
      pin = await this.newPin()
      if (!pin) return
    }
    const password = await this.currentPassword()
    if (password === undefined) return
    try {
      await backend.vaultSetWay(way, password, newPassword, pin)
      this.message = t(`settings.waySet.${way}`)
    } catch (err) {
      if (!isBackendError(err) || err.kind !== 'cancelled') this.message = errorText(err)
    }
    await this.refresh()
  }

  /** An old unencrypted vault: encrypted, with a new password. */
  async encrypt() {
    this.message = ''
    const password = await this.newPassword()
    if (!password) return
    try {
      await backend.vaultEncrypt()
      await backend.vaultSetWay('password', null, password)
      this.message = t('settings.encrypted')
    } catch (err) {
      this.message = errorText(err)
    }
    await this.refresh()
  }

  async createRecovery() {
    this.message = ''
    const password = await this.currentPassword()
    if (password === undefined) return
    try {
      this.recoveryCode = await backend.vaultCreateRecovery(password)
    } catch (err) {
      this.message = errorText(err)
    }
  }

  async removeRecovery() {
    this.message = ''
    const password = await this.currentPassword()
    if (password === undefined) return
    try {
      await backend.vaultRemoveRecovery(password)
      await this.refresh()
    } catch (err) {
      this.message = errorText(err)
    }
  }

  /** The recovery key was shown and saved. */
  async recoveryShown() {
    this.recoveryCode = ''
    await this.refresh()
  }

  async changePassword(old: string, next: string): Promise<boolean> {
    this.message = ''
    try {
      await backend.vaultChangePassword(old, next)
      this.message = t('settings.passwordChanged')
      return true
    } catch (err) {
      this.message = errorText(err)
      return false
    }
  }

  /** The same change through two dialogs, for the phone layout. */
  async changePasswordAsked() {
    const old = await overlay.askPassword(t('settings.oldPassword'))
    if (!old) return
    const next = await overlay.askPassword(t('settings.newPassword'))
    if (!next) return
    if (next.length < 8) {
      this.message = t('setup.tooShort', { count: 8 })
      return
    }
    await this.changePassword(old, next)
  }

  async exportNotes() {
    const dir = await pickToOpen({ directory: true, title: t('settings.exportPick') })
    if (dir === null) return
    try {
      const report = await backend.exportMarkdown(dir)
      this.message = t('settings.exported', { count: report.notes })
    } catch (err) {
      this.message = errorText(err)
    }
  }

  /** The same export as one zip file. */
  async exportZip() {
    const path = await pickToSave({
      title: t('settings.exportZip'),
      defaultPath: 'notes.zip',
      filters: [{ name: 'zip', extensions: ['zip'] }],
    })
    if (path === null) return
    try {
      const report = await backend.exportMarkdownZip(path)
      this.message = t('settings.exported', { count: report.notes })
    } catch (err) {
      this.message = errorText(err)
    }
  }
}
