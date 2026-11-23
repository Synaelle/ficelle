// SPDX-License-Identifier: AGPL-3.0-or-later
// What the desktop dialog and the phone settings share: the vault status and
// the actions that change it. Each view only lays them out its own way.

import { channelChoice, updatesHere } from '../lib/appUpdate.svelte'
import type { IconName } from '../lib/icons/sets'
import { HANDHELD } from '../lib/platform'
import { MIN_PASSWORD } from '../lib/passwords'
import { encryptFlow } from '../lib/encryptFlow.svelte'
import { backend, PIN_LENGTH, type VaultStatus, type VaultWay } from '../lib/backend'
import { backups } from '../lib/backups.svelte'
import { t } from '../lib/i18n.svelte'
import { pickToOpen, pickToSave } from '../lib/pick'
import { overlay } from '../lib/overlay.svelte'
import { errorKind, shownError, type ShownError } from '../lib/errors'

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

/** The pages a setting sits on, by the phone's names: the desktop's Data
 * section is the phone's notes page. */
export type InsidePage =
  'appearance' | 'accessibility' | 'editor' | 'language' | 'notes' | 'updates' | 'about'

/** Settings inside the pages, found by their own name in a search, with
 * the page they sit on. */
export const SETTINGS_INSIDE: (readonly [IconName, string, InsidePage])[] = [
  ['eye', 'settings.theme', 'appearance'],
  ['backup', 'backups.title', 'notes'],
  ['restore', 'backups.restoreFrom', 'notes'],
  ['import', 'settings.importTitle', 'notes'],
  ['export', 'settings.exportTitle', 'notes'],
  ['palette', 'settings.pureBlack', 'appearance'],
  ['textSize', 'settings.uiSize', 'accessibility'],
  ['font', 'settings.dyslexicFont', 'accessibility'],
  ['textStyle', 'settings.lineSpacing', 'editor'],
  ['list', 'settings.autoLists', 'editor'],
  ['emoji', 'settings.emojiShortcodes', 'editor'],
  ['allNotes', 'settings.openLastNote', 'editor'],
  ['search', 'settings.scopedSearch', 'editor'],
  ['attachment', 'settings.nameFromAttachment', 'editor'],
  ['right', 'settings.arrow', 'editor'],
  ['timer', 'settings.clock', 'language'],
  ['info', 'settings.credits', 'about'],
  // a phone spells with its system and never lists beside the note
  ...(HANDHELD
    ? []
    : ([
        ['pencil', 'settings.spellcheck', 'editor'],
        ['pin', 'settings.openNoteFirst', 'editor'],
      ] as const)),
  ...(updatesHere ? ([['search', 'appUpdate.check', 'updates']] as const) : []),
  ...(updatesHere && channelChoice
    ? ([['flask', 'appUpdate.nightlyVersions', 'updates']] as const)
    : []),
]

export class SettingsState {
  vault: VaultStatus | null = $state(null)
  /** Last outcome to show, success or error, already translated. */
  message: ShownError | string = $state('')
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
      if (errorKind(err) !== 'cancelled') this.message = shownError(err, 'SECURITY')
    }
    await this.refresh()
  }

  /** Unencrypted notes being encrypted: the setup's password step shows
   * over everything (App.svelte), with its explanation, a second field,
   * the recovery key and the quicker ways in. */
  encrypt() {
    this.message = ''
    encryptFlow.start((done) => {
      if (done) this.message = t('settings.encrypted')
      void this.refresh()
    })
  }

  async createRecovery() {
    this.message = ''
    const password = await this.currentPassword()
    if (password === undefined) return
    try {
      this.recoveryCode = await backend.vaultCreateRecovery(password)
    } catch (err) {
      this.message = shownError(err, 'RECOVERY')
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
      this.message = shownError(err, 'RECOVERY')
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
      void backups.runAfterPasswordChange()
      return true
    } catch (err) {
      this.message = shownError(err, 'PASSWORD')
      return false
    }
  }

  /** The same change through two dialogs, for the phone layout. */
  async changePasswordAsked() {
    const old = await overlay.askPassword(t('settings.oldPassword'))
    if (!old) return
    const next = await overlay.askPassword(t('settings.newPassword'))
    if (!next) return
    if (next.length < MIN_PASSWORD) {
      this.message = t('setup.tooShort', { count: MIN_PASSWORD })
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
      this.message = shownError(err, 'EXPORT')
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
      this.message = shownError(err, 'EXPORT')
    }
  }
}
