// SPDX-License-Identifier: AGPL-3.0-or-later
// Backend over tauri commands. Names match src-tauri/src/commands.

import { invoke } from '@tauri-apps/api/core'
import { ANDROID } from '../platform'
import type { Backend } from './backend'

const isWindowsLike = /windows|android/i.test(navigator.userAgent)

/** The knot, not Android's "i", and largeBody because alone in its list a
 *  notification shows opened, where a plain body is left out. The plugin
 *  panics at startup on any config, so these go with each notification. */
const ANDROID_NOTIFICATION = (body: string) => ({
  largeBody: body,
  icon: 'ic_notification',
  iconColor: '#E44C3E',
})

/** Url of a file by its hash, served decrypted by the native side. */
const fileUrl = (sha256: string) =>
  isWindowsLike ? `http://attachment.localhost/${sha256}` : `attachment://localhost/${sha256}`

/** The backend over `call`, the native side's `invoke`, or the web
 * version's worker that answers the same commands. */
export function createTauriBackend(call: typeof invoke = invoke): Backend {
  return {
    native: true,
    vaultStatus: () => call('vault_status'),
    vaultCreate: (way, password, pin) => call('vault_create', { way, password, pin }),
    vaultUnlock: (password) => call('vault_unlock', { password }),
    vaultUnlockPin: (pin) => call('vault_unlock_pin', { pin }),
    vaultUnlockBiometric: () => call('vault_unlock_biometric'),
    vaultSetWay: (way, password, newPassword, pin) =>
      call('vault_set_way', { way, password, newPassword, pin }),
    vaultEncrypt: (password) => call('vault_encrypt', { password }),
    appUpdateCheck: (channel, allowOlder) => call('app_update_check', { channel, allowOlder }),
    appUpdateLatest: (channel, allowOlder) => call('app_update_latest', { channel, allowOlder }),
    appUpdateInstall: () => call('app_update_install'),
    appUpdatePageOnly: () => call('app_update_page_only'),
    notificationsAllowed: async () => {
      const { isPermissionGranted, requestPermission } =
        await import('@tauri-apps/plugin-notification')
      return (await isPermissionGranted()) || (await requestPermission()) === 'granted'
    },
    notify: (title, body) =>
      call('plugin:notification|notify', {
        options: { title, body, ...(ANDROID ? ANDROID_NOTIFICATION(body) : {}) },
      }),
    quickWindowOpen: (title) => call('quick_window_open', { title }),
    quickWindowReady: (title) => call('quick_window_ready', { title }),
    fontDownload: (id, files) => call('font_download', { id, files }),
    fontFiles: (id, files) => call('font_files', { id, files }),
    fontFile: (id, name) => call('font_file', { id, name }),
    systemClock24h: () => call('system_clock_24h'),
    clipboardImages: () => call('clipboard_images'),
    clipboardImage: (index) => call('clipboard_image', { index }),
    droppedImages: () => call('dropped_images'),
    droppedImage: (index) => call('dropped_image', { index }),
    vaultCheckPassword: (password) => call('vault_check_password', { password }),
    vaultCreateRecovery: (password) => call('vault_create_recovery', { password }),
    vaultRemoveRecovery: (password) => call('vault_remove_recovery', { password }),
    vaultRecover: (code, newPassword) => call('vault_recover', { code, newPassword }),
    vaultLock: () => call('vault_lock'),
    vaultReset: () => call('vault_reset'),
    vaultChangePassword: (old, next) => call('vault_change_password', { old, new: next }),

    listNotes: (scope, sort) => call('list_notes', { scope, sort }),
    getNote: (id) => call('get_note', { id }),
    createNote: (note) => call('create_note', { note }),
    updateNote: (id, change) => call('update_note', { id, change }),
    setPinned: (id, pinned) => call('set_pinned', { id, pinned }),
    moveNote: (id, folder) => call('move_note', { id, folder }),
    setNoteTags: (id, tags) => call('set_note_tags', { id, tags }),
    trashNote: (id) => call('trash_note', { id }),
    restoreNote: (id) => call('restore_note', { id }),
    purgeNote: (id) => call('purge_note', { id }),
    discardIfBlank: (id) => call('discard_if_blank', { id }),
    emptyTrash: () => call('empty_trash'),

    listFolders: () => call('list_folders'),
    createFolder: (parent, name) => call('create_folder', { parent, name }),
    renameFolder: (id, name) => call('rename_folder', { id, name }),
    moveFolder: (id, parent) => call('move_folder', { id, parent }),
    trashFolder: (id) => call('trash_folder', { id }),
    listTrashedFolders: () => call('list_trashed_folders'),
    restoreFolder: (id) => call('restore_folder', { id }),

    listTags: () => call('list_tags'),
    createTag: (name) => call('create_tag', { name }),
    renameTag: (id, name) => call('rename_tag', { id, name }),
    setTagColor: (id, color) => call('set_tag_color', { id, color }),
    deleteTag: (id) => call('delete_tag', { id }),
    tagsMerged: () => call('tags_merged'),
    syncedSettings: () => call('synced_settings'),
    setSyncedSetting: (key, value) => call('set_synced_setting', { key, value }),
    tagsMergedSeen: () => call('tags_merged_seen'),
    listVersions: (id) => call('list_versions', { id }),
    restoreVersion: (id) => call('restore_version', { id }),
    noteConflict: (id) => call('note_conflict', { id }),
    conflictedNotes: () => call('conflicted_notes'),
    resolveConflict: (id, keep) => call('resolve_conflict', { id, keep }),

    search: (query, limit) => call('search', { query, limit }),
    backlinks: (id) => call('backlinks', { id }),
    resolveLink: (target) => call('resolve_link', { target }),
    linkSuggestions: (input, limit) => call('link_suggestions', { input, limit }),
    noteHeadings: (id) => call('note_headings', { id }),

    // raw body with metadata in headers, see add_attachment in files.rs
    addAttachment: async (note, file) =>
      call('add_attachment', new Uint8Array(await file.arrayBuffer()), {
        headers: {
          'x-note-id': note,
          'x-file-name': encodeURIComponent(file.name),
          'x-mime-type': file.type || 'application/octet-stream',
        },
      }),
    attachInboxFile: (note, path, name, mime) =>
      call('attach_inbox_file', { note, path, name, mime }),
    listAttachments: (id) => call('list_attachments', { id }),
    allAttachments: () => call('all_attachments'),
    removeAttachment: (id) => call('remove_attachment', { id }),
    fileUrl,
    attachmentFiles: (note) => call('attachment_files', { note }),
    fetchAttachment: (sha256) => call('fetch_attachment', { sha256 }),
    saveAttachment: (sha256, path) => call('save_attachment', { sha256, path }),
    readAttachment: (sha256) => call('read_attachment', { sha256 }),
    // raw body as for add_attachment
    setAttachmentThumb: async (id, thumb) =>
      call('set_attachment_thumb', new Uint8Array(await thumb.arrayBuffer()), {
        headers: { 'x-attachment-id': id },
      }),
    filesInfo: () => call('files_info'),
    setAutoFetch: (mb) => call('files_set_auto_fetch', { mb }),
    exportMarkdown: (dir) => call('export_markdown', { dir }),
    exportMarkdownZip: (path) => call('export_markdown_zip', { path }),

    backupDefaultName: () => call('backup_default_name'),
    backupWrite: (path, password) => call('backup_write', { path, password }),
    backupEncrypted: (path) => call('backup_encrypted', { path }),
    backupInspect: (path, password) => call('backup_inspect', { path, password }),
    backupRestore: (path, password) => call('backup_restore', { path, password }),

    importOpen: (path) => call('import_open', { path }),
    importRead: async (id, path) =>
      new Uint8Array(await call<ArrayBuffer>('import_read', { id, path })),
    importKnown: (importer, keys) => call('import_known', { importer, keys }),
    importBegin: (id, name) => call('import_begin', { id, name }),
    // raw body as for add_attachment
    importPut: (bytes) => call('import_put', bytes),
    importWrite: (id, batch) => call('import_write', { id, batch }),
    importClose: (id) => call('import_close', { id }),

    backupsGet: () => call('backups_get'),
    backupsFound: (target) => call('backups_found', { target }),
    backupsPickFolder: () => call('backups_pick_folder'),
    backupsConnect: (target, password, create, host) =>
      call('backups_connect', { target, password, create, host }),
    backupsSettings: (schedule, policy) => call('backups_settings', { schedule, policy }),
    backupsDisconnect: () => call('backups_disconnect'),
    backupsRun: (offsetSeconds) => call('backups_run', { offsetSeconds }),
    backupsSnapshots: () => call('backups_snapshots'),
    backupsRestore: (snapshot) => call('backups_restore', { snapshot }),
    backupsTestRestore: (snapshot) => call('backups_test_restore', { snapshot }),
    backupsCheck: (full) => call('backups_check', { full }),
    feedbackWhere: () => call('feedback_where'),
    feedbackSend: (draft) => call('feedback_send', { draft }),
    feedbackAnswers: (url, secrets) => call('feedback_answers', { url, secrets }),
  }
}
