// SPDX-License-Identifier: AGPL-3.0-or-later
// Everything the ui needs from storage. The native app implements it with
// tauri commands, the browser preview and tests with an in-memory store.

import type {
  Attachment,
  ExportSummary,
  Folder,
  Id,
  LinkSuggestion,
  NewNote,
  Note,
  NoteChange,
  NoteConflict,
  NoteSort,
  NoteSummary,
  NoteVersion,
  Resolution,
  Scope,
  SearchHit,
  Tag,
  TagColor,
  VaultStatus,
  VaultWay,
  AppUpdate,
  AttachmentFile,
  FilesInfo,
  BackupManifest,
  ImportBatchInput,
  ImportOpened,
  ImportWritten,
  RestoreReport,
  UpdateChannel,
} from './types'

export interface Backend {
  /** True when attachments and export are backed by real files. */
  readonly native: boolean

  vaultStatus(): Promise<VaultStatus>
  /** First launch: the password, and how the vault opens day to day (`pin`
   * for the PIN way). `plain` with no password only in the browser's tests. */
  vaultCreate(way: VaultWay, password: string | null, pin?: string | null): Promise<void>
  /** `password` null opens an old vault this device opened alone. */
  vaultUnlock(password: string | null): Promise<void>
  /** `wrongPassword` for a wrong PIN with tries left, `notRemembered` once
   * they ran out. */
  vaultUnlockPin(pin: string): Promise<void>
  /** Unlocks after Windows Hello or the platform equivalent. */
  vaultUnlockBiometric(): Promise<void>
  /** Changes how the vault opens day to day. `password` proves the right to
   * (Windows Hello is asked otherwise), `newPassword` gives an old vault its
   * password, `pin` is the PIN way's. */
  vaultSetWay(
    way: VaultWay,
    password: string | null,
    newPassword: string | null,
    pin?: string | null,
  ): Promise<void>
  /** An unencrypted vault, encrypted with `password` (no system keychain
   * needed), or without one with a key this device keeps until it is given
   * a password. */
  vaultEncrypt(password: string | null): Promise<void>
  /** A newer version of the app, downloaded and ready, or only found with
   * its download page (`page`). `null`: up to date, or this
   * build does not update itself (dev, preview, the browser). `channel`
   * picks the Stable or the Beta list. `allowOlder`: a beta going back to
   * Stable, an older version with the same database schema comes too. */
  appUpdateCheck(channel: UpdateChannel, allowOlder?: boolean): Promise<AppUpdate | null>
  /** The newest version on the release list, downloading nothing: with
   * automatic updates off, looked at only for a critical security fix. */
  appUpdateLatest(channel: UpdateChannel, allowOlder?: boolean): Promise<AppUpdate | null>
  /** Desktop: installs the downloaded update and starts the app again. The
   * portable exe, a Linux package, and a Mac app with nothing downloaded,
   * open the version's release page instead. */
  appUpdateInstall(): Promise<void>
  /** This build only points to the release page: the portable exe, a Linux
   * package, a Mac app that cannot replace itself. */
  appUpdatePageOnly(): Promise<boolean>
  /** The system lets this app show notifications, asking the person if it
   * must. False where there are none (the browser). */
  notificationsAllowed(): Promise<boolean>
  /** A system notification. */
  notify(title: string, body: string): Promise<void>
  /** Desktop: opens the quick note window (commands/quick.rs). */
  quickWindowOpen(title: string): Promise<void>
  /** The quick note page is drawn: its window shows, in front. */
  quickWindowReady(title: string): Promise<void>
  /** Downloads a font's files (commands/fonts.rs), returns those it has.
   * `files` defaults to the usual eight, see `FILES` there. */
  fontDownload(id: string, files?: string[]): Promise<string[]>
  /** The files of a font this device has, of `files` or the usual eight. */
  fontFiles(id: string, files?: string[]): Promise<string[]>
  fontFile(id: string, name: string): Promise<ArrayBuffer>
  /** The system shows a 24-hour clock, `null` when it does not say. */
  systemClock24h(): Promise<boolean | null>
  /** Images on the system clipboard: copied image files, or a copied picture. */
  clipboardImages(): Promise<{ name: string; mime: string }[]>
  /** One of `clipboardImages`, read again from the clipboard. */
  clipboardImage(index: number): Promise<ArrayBuffer>
  /** The image files of the last drop on the window, which the system named. */
  droppedImages(): Promise<{ name: string; mime: string }[]>
  /** One of `droppedImages`. */
  droppedImage(index: number): Promise<ArrayBuffer>
  /** Returns the new key to show once, any earlier key stops working. */
  /** Whether `password` opens these notes, nothing else done. */
  vaultCheckPassword(password: string): Promise<boolean>
  vaultCreateRecovery(password: string | null): Promise<string>
  vaultRemoveRecovery(password: string | null): Promise<void>
  /** Opens with the recovery key and sets a new password. */
  vaultRecover(code: string, newPassword: string): Promise<void>
  vaultLock(): Promise<void>
  /** Deletes everything on this device, locked or not: the first launch setup comes next. */
  vaultReset(): Promise<void>
  vaultChangePassword(old: string, next: string): Promise<void>

  listNotes(scope: Scope, sort: NoteSort): Promise<NoteSummary[]>
  getNote(id: Id): Promise<Note>
  createNote(note: NewNote): Promise<Note>
  updateNote(id: Id, change: NoteChange): Promise<Note>
  setPinned(id: Id, pinned: boolean): Promise<void>
  /** Returns the note's id, a new one when it moved into another space. */
  moveNote(id: Id, folder: Id | null): Promise<Id>
  setNoteTags(id: Id, tags: string[]): Promise<void>
  trashNote(id: Id): Promise<void>
  restoreNote(id: Id): Promise<void>
  purgeNote(id: Id): Promise<void>
  /** Deletes the note if it has no title, no text and no pin, true when it did. */
  discardIfBlank(id: Id): Promise<boolean>
  emptyTrash(): Promise<number>

  listFolders(): Promise<Folder[]>
  createFolder(parent: Id | null, name: string): Promise<Folder>
  renameFolder(id: Id, name: string): Promise<void>
  moveFolder(id: Id, parent: Id | null): Promise<void>
  trashFolder(id: Id): Promise<void>
  /** Folders in the trash that can be restored, those trashed with a parent come with it. */
  listTrashedFolders(): Promise<Folder[]>
  restoreFolder(id: Id): Promise<void>

  listTags(): Promise<Tag[]>
  /** A tag kept without notes, with every tag above it. */
  createTag(name: string): Promise<Id>
  /** Renames a tag or moves it with a path, the tags it holds go along. */
  renameTag(id: Id, name: string): Promise<void>
  setTagColor(id: Id, color: TagColor | null): Promise<void>
  /** Takes a tag and the tags it holds off every note. */
  deleteTag(id: Id): Promise<void>
  /** The choices made on some device that follow the person to every
   * device, by name, each a JSON value. */
  syncedSettings(): Promise<[string, string][]>
  setSyncedSetting(key: string, value: string): Promise<void>
  /** This vault's folders became tags and the person was not told yet. */
  tagsMerged(): Promise<boolean>
  tagsMergedSeen(): Promise<void>
  listVersions(id: Id): Promise<NoteVersion[]>
  /** What a note waits for after a sync, null when nothing. */
  noteConflict(id: Id): Promise<NoteConflict | null>
  conflictedNotes(): Promise<Id[]>
  resolveConflict(id: Id, keep: Resolution): Promise<void>
  restoreVersion(id: Id): Promise<Note>

  search(query: string, limit: number): Promise<SearchHit[]>
  backlinks(id: Id): Promise<NoteSummary[]>
  resolveLink(target: string): Promise<Id | null>
  linkSuggestions(input: string, limit: number): Promise<LinkSuggestion[]>
  noteHeadings(id: Id): Promise<string[]>

  addAttachment(note: Id, file: File): Promise<Attachment>
  /** Attaches a file another app shared, copied into the app's inbox folder. */
  attachInboxFile(note: Id, path: string, name: string, mime: string): Promise<Attachment>
  listAttachments(note: Id): Promise<Attachment[]>
  /** Every attachment of the notes not in the trash, newest first. */
  allAttachments(): Promise<import('./types').NoteAttachment[]>
  removeAttachment(id: Id): Promise<void>
  /** Url the webview can load for a file by its hash (a preview). */
  fileUrl(sha256: string): string
  /** A note's attachments with where each file stands here. */
  attachmentFiles(note: Id): Promise<AttachmentFile[]>
  /** Fetches a file from the server now; `files-changed` follows. */
  fetchAttachment(sha256: string): Promise<void>
  /** Writes a copy of a file where the person chose. */
  saveAttachment(sha256: string, path: string): Promise<void>
  /** A file kept on this device, as bytes: an image to copy. */
  readAttachment(sha256: string): Promise<ArrayBuffer>
  /** Gives an image attachment its small preview. */
  setAttachmentThumb(id: Id, thumb: Blob): Promise<void>
  /** The server's rules and usage for this account, null without one. */
  filesInfo(): Promise<FilesInfo | null>
  /** Files up to this many megabytes come with each sync. */
  setAutoFetch(mb: number): Promise<void>
  exportMarkdown(dir: string): Promise<ExportSummary>
  /** The same export as one zip file. */
  exportMarkdownZip(path: string): Promise<ExportSummary>

  /** The name offered for a new backup. */
  backupDefaultName(): Promise<string>
  /** Writes a backup, sealed with the password when there is one. */
  backupWrite(path: string, password: string | null): Promise<BackupManifest>
  /** Whether a backup needs its password. */
  backupEncrypted(path: string): Promise<boolean>
  /** What a backup holds and how many of its notes are here already. */
  backupInspect(
    path: string,
    password: string | null,
  ): Promise<{ manifest: BackupManifest; present: number }>
  backupRestore(path: string, password: string | null): Promise<RestoreReport>

  /** Opens a folder, an archive or a file to import from (the app only). */
  importOpen(path: string): Promise<ImportOpened>
  importRead(source: number, path: string): Promise<Uint8Array>
  /** The keys among these an earlier import by `importer` brought in. */
  importKnown(importer: string, keys: string[]): Promise<string[]>
  /** Makes the import's own folder. `source` is null for exports in memory. */
  importBegin(source: number | null, name: string): Promise<Id>
  /** Stores bytes decoded by an importer, attached by a later batch. */
  importPut(bytes: Uint8Array): Promise<{ sha256: string; size: number }>
  importWrite(source: number | null, batch: ImportBatchInput): Promise<ImportWritten>
  importClose(source: number): Promise<void>

  /** Where a feedback report would go now. */
  feedbackWhere(): Promise<import('./types').FeedbackWhere>
  /** Sends a report there, the proof of work done natively. */
  feedbackSend(draft: import('./types').FeedbackDraft): Promise<import('./types').FeedbackSent>
  /** How the reports sent to `url` with these secrets stand; one it no longer has is left out. */
  feedbackAnswers(
    url: string,
    secrets: string[],
  ): Promise<{ reports: import('./types').FeedbackAnswer[] }>
  /** This device's scheduled backups. */
  backupsGet(): Promise<import('./types').BackupsView>
  /** Whether `target` already holds a backup and needs its password, nothing written. */
  backupsFound(target: import('./types').BackupTarget): Promise<import('./types').BackupsFound>
  /** A phone's backups folder through the system's picker, which lets the app
   * reach what an earlier install made there: its address, null when the
   * person backed out. */
  backupsPickFolder(): Promise<string | null>
  /** Sets backups up: a new repository at `target`, or one there opened with
   * its password. An empty password stands for backups without one. */
  backupsConnect(
    target: import('./types').BackupTarget,
    password: string,
    create: boolean,
    host: string,
  ): Promise<import('./types').BackupsView>
  backupsSettings(
    schedule: import('./types').BackupSchedule,
    policy: import('./types').BackupPolicy,
  ): Promise<import('./types').BackupsView>
  /** Stops backing up here, the repository stays. */
  backupsDisconnect(): Promise<import('./types').BackupsView>
  /** Backs up now, `offsetSeconds` the time zone for the policy's days. */
  backupsRun(offsetSeconds: number): Promise<import('./types').BackupsView>
  /** Snapshots of the repository, newest first. */
  backupsSnapshots(): Promise<import('./types').BackupSnapshot[]>
  backupsRestore(snapshot: string): Promise<import('./types').RestoreReport>
  backupsTestRestore(snapshot: string): Promise<import('./types').BackupTestRestore>
  backupsCheck(full: boolean): Promise<import('./types').BackupCheck>
}
