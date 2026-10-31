// SPDX-License-Identifier: AGPL-3.0-or-later
// Backend over tauri commands. Names match src-tauri/src/commands.

import { invoke } from '@tauri-apps/api/core'
import type { Backend } from './backend'
import type { Attachment } from './types'

const isWindowsLike = /windows|android/i.test(navigator.userAgent)

/** Url of a file by its hash, served decrypted by the native side. */
const fileUrl = (sha256: string) =>
  isWindowsLike ? `http://attachment.localhost/${sha256}` : `attachment://localhost/${sha256}`

export function createTauriBackend(): Backend {
  return {
    native: true,
    vaultStatus: () => invoke('vault_status'),
    vaultCreate: (way, password, pin) => invoke('vault_create', { way, password, pin }),
    vaultUnlock: (password) => invoke('vault_unlock', { password }),
    vaultUnlockPin: (pin) => invoke('vault_unlock_pin', { pin }),
    vaultUnlockBiometric: () => invoke('vault_unlock_biometric'),
    vaultSetWay: (way, password, newPassword, pin) =>
      invoke('vault_set_way', { way, password, newPassword, pin }),
    vaultEncrypt: () => invoke('vault_encrypt'),
    appUpdateCheck: () => invoke('app_update_check'),
    appUpdateLatest: () => invoke('app_update_latest'),
    appUpdateInstall: () => invoke('app_update_install'),
    quickWindowOpen: (title) => invoke('quick_window_open', { title }),
    fontDownload: (id, files) => invoke('font_download', { id, files }),
    fontFiles: (id, files) => invoke('font_files', { id, files }),
    fontFile: (id, name) => invoke('font_file', { id, name }),
    systemFonts: () => invoke('system_fonts'),
    systemAccent: () => invoke('system_accent'),
    systemClock24h: () => invoke('system_clock_24h'),
    vaultCreateRecovery: (password) => invoke('vault_create_recovery', { password }),
    vaultRemoveRecovery: (password) => invoke('vault_remove_recovery', { password }),
    vaultRecover: (code, newPassword) => invoke('vault_recover', { code, newPassword }),
    vaultLock: () => invoke('vault_lock'),
    vaultReset: () => invoke('vault_reset'),
    vaultChangePassword: (old, next) => invoke('vault_change_password', { old, new: next }),

    listNotes: (scope, sort) => invoke('list_notes', { scope, sort }),
    getNote: (id) => invoke('get_note', { id }),
    createNote: (note) => invoke('create_note', { note }),
    updateNote: (id, change) => invoke('update_note', { id, change }),
    setPinned: (id, pinned) => invoke('set_pinned', { id, pinned }),
    moveNote: (id, folder) => invoke('move_note', { id, folder }),
    setNoteTags: (id, tags) => invoke('set_note_tags', { id, tags }),
    trashNote: (id) => invoke('trash_note', { id }),
    restoreNote: (id) => invoke('restore_note', { id }),
    purgeNote: (id) => invoke('purge_note', { id }),
    discardIfBlank: (id) => invoke('discard_if_blank', { id }),
    emptyTrash: () => invoke('empty_trash'),

    listFolders: () => invoke('list_folders'),
    createFolder: (parent, name) => invoke('create_folder', { parent, name }),
    renameFolder: (id, name) => invoke('rename_folder', { id, name }),
    moveFolder: (id, parent) => invoke('move_folder', { id, parent }),
    trashFolder: (id) => invoke('trash_folder', { id }),
    restoreFolder: (id) => invoke('restore_folder', { id }),

    listTags: () => invoke('list_tags'),
    listVersions: (id) => invoke('list_versions', { id }),
    restoreVersion: (id) => invoke('restore_version', { id }),
    noteConflict: (id) => invoke('note_conflict', { id }),
    conflictedNotes: () => invoke('conflicted_notes'),
    resolveConflict: (id, keep) => invoke('resolve_conflict', { id, keep }),

    search: (query, limit) => invoke('search', { query, limit }),
    backlinks: (id) => invoke('backlinks', { id }),
    resolveLink: (target) => invoke('resolve_link', { target }),
    linkSuggestions: (input, limit) => invoke('link_suggestions', { input, limit }),
    noteHeadings: (id) => invoke('note_headings', { id }),

    // raw body with metadata in headers, see add_attachment in files.rs
    addAttachment: async (note, file) =>
      invoke('add_attachment', new Uint8Array(await file.arrayBuffer()), {
        headers: {
          'x-note-id': note,
          'x-file-name': encodeURIComponent(file.name),
          'x-mime-type': file.type || 'application/octet-stream',
        },
      }),
    attachInboxFile: (note, path, name, mime) =>
      invoke('attach_inbox_file', { note, path, name, mime }),
    listAttachments: (id) => invoke('list_attachments', { id }),
    allAttachments: () => invoke('all_attachments'),
    removeAttachment: (id) => invoke('remove_attachment', { id }),
    attachmentUrl: (a: Attachment) => fileUrl(a.sha256),
    fileUrl,
    attachmentFiles: (note) => invoke('attachment_files', { note }),
    fetchAttachment: (sha256) => invoke('fetch_attachment', { sha256 }),
    saveAttachment: (sha256, path) => invoke('save_attachment', { sha256, path }),
    // raw body as for add_attachment
    setAttachmentThumb: async (id, thumb) =>
      invoke('set_attachment_thumb', new Uint8Array(await thumb.arrayBuffer()), {
        headers: { 'x-attachment-id': id },
      }),
    filesInfo: () => invoke('files_info'),
    setAutoFetch: (mb) => invoke('files_set_auto_fetch', { mb }),
    exportMarkdown: (dir) => invoke('export_markdown', { dir }),
    exportMarkdownZip: (path) => invoke('export_markdown_zip', { path }),

    backupDefaultName: () => invoke('backup_default_name'),
    backupWrite: (path, password) => invoke('backup_write', { path, password }),
    backupEncrypted: (path) => invoke('backup_encrypted', { path }),
    backupInspect: (path, password) => invoke('backup_inspect', { path, password }),
    backupRestore: (path, password) => invoke('backup_restore', { path, password }),

    importOpen: (path) => invoke('import_open', { path }),
    importRead: async (id, path) =>
      new Uint8Array(await invoke<ArrayBuffer>('import_read', { id, path })),
    importKnown: (importer, keys) => invoke('import_known', { importer, keys }),
    importBegin: (id, name) => invoke('import_begin', { id, name }),
    // raw body as for add_attachment
    importPut: (bytes) => invoke('import_put', bytes),
    importWrite: (id, batch) => invoke('import_write', { id, batch }),
    importClose: (id) => invoke('import_close', { id }),

    themesList: () => invoke('themes_list'),
    themeSave: (json) => invoke('theme_save', { json }),
    themeRemove: (id) => invoke('theme_remove', { id }),
    readTextFile: (path) => invoke('read_text_file', { path }),
    writeTextFile: (path, text) => invoke('write_text_file', { path, text }),

    instanceExtensions: () => invoke('instance_extensions'),
    backupsGet: () => invoke('backups_get'),
    backupsConnect: (target, password, create, host) =>
      invoke('backups_connect', { target, password, create, host }),
    backupsSettings: (schedule, policy) => invoke('backups_settings', { schedule, policy }),
    backupsDisconnect: () => invoke('backups_disconnect'),
    backupsRun: (offsetSeconds) => invoke('backups_run', { offsetSeconds }),
    backupsSnapshots: () => invoke('backups_snapshots'),
    backupsRestore: (snapshot) => invoke('backups_restore', { snapshot }),
    backupsTestRestore: (snapshot) => invoke('backups_test_restore', { snapshot }),
    backupsCheck: (full) => invoke('backups_check', { full }),
    catalogList: (direct) => invoke('catalog_list', { direct }),
    catalogTheme: (id, direct) => invoke('catalog_theme', { id, direct }),
    feedbackWhere: () => invoke('feedback_where'),
    feedbackSend: (draft) => invoke('feedback_send', { draft }),
    feedbackAnswers: (url, secrets) => invoke('feedback_answers', { url, secrets }),
    instanceTheme: (id) => invoke('instance_theme', { id }),
  }
}
