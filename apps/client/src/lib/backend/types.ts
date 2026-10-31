// SPDX-License-Identifier: AGPL-3.0-or-later
// Mirrors of the core types as they cross the bridge (serde camelCase).
// Keep in sync with crates/core/src/model.rs and store/notes.rs.

export type Id = string
export type Timestamp = number

export interface Note {
  id: Id
  folderId: Id | null
  title: string
  body: string
  pinned: boolean
  tags: string[]
  createdAt: Timestamp
  updatedAt: Timestamp
  trashedAt: Timestamp | null
  /** The space it belongs to: empty for the personal space, else a shared space's id. */
  spaceId: string
}

export interface NoteSummary {
  id: Id
  folderId: Id | null
  title: string
  snippet: string
  pinned: boolean
  updatedAt: Timestamp
  createdAt: Timestamp
  /** As `Note.spaceId`. */
  spaceId: string
}

export interface Folder {
  id: Id
  parentId: Id | null
  name: string
  position: number
  createdAt: Timestamp
  updatedAt: Timestamp
  trashedAt: Timestamp | null
  /** The space it belongs to, empty for the personal space. */
  spaceId: string
}

export interface Tag {
  id: Id
  name: string
  noteCount: number
}

export interface NoteVersion {
  id: Id
  noteId: Id
  title: string
  body: string
  createdAt: Timestamp
  reason: 'edit' | 'beforeRestore' | 'mineBeforeMerge' | 'theirsBeforeMerge'
}

/** A note waiting for a decision after a sync. */
export interface NoteConflict {
  noteId: Id
  /** Rewritten on two devices at once, or edited elsewhere while deleted here. */
  kind: 'merge' | 'editedAfterDelete'
  /** This device's text before the merge. */
  mine: { title: string; body: string } | null
  /** The other device's text before the merge. */
  theirs: { title: string; body: string } | null
  at: Timestamp
}

/** The way out of a conflict. */
export type Resolution =
  { keep: 'merged' } | { keep: 'mine' } | { keep: 'theirs' } | { keep: 'both'; copyTitle: string }

export interface Attachment {
  id: Id
  noteId: Id
  fileName: string
  mimeType: string
  size: number
  sha256: string
  createdAt: Timestamp
  /** A small preview of an image, by hash. */
  thumbSha256: string | null
}

/** An attachment with its note's title, for the list of every file. */
export interface NoteAttachment extends Attachment {
  noteTitle: string
}

/** An attachment and where its file stands on this device. */
export interface AttachmentFile extends Attachment {
  /** The file is here. */
  here: boolean
  /** The server refused it: an error code such as `fileTooLarge`. */
  refused: string | null
}

/** The account's rules for files, sizes in bytes, 0 for no limit. */
export interface FilesPolicy {
  maxFileBytes: number
  quotaBytes: number
  allowedTypes: string[]
  blockedTypes: string[]
  maxFilesPerNote: number
  imageMaxPx: number
  imageQuality: number
  thumbnails: boolean
  retentionDays: number
}

/** What the server allows this account and what it uses. */
export interface FilesInfo {
  policy: FilesPolicy
  usage: { usedBytes: number; fileBytes: number; files: number }
  partBytes: number
}

export interface SearchHit {
  noteId: Id
  title: string
  /** Matches are wrapped in U+0002 and U+0003. */
  snippet: string
}

export interface LinkSuggestion {
  id: Id
  title: string
}

export type NoteSort = 'updatedDesc' | 'createdDesc' | 'titleAsc'

export type Scope =
  { kind: 'all' } | { kind: 'folder'; id: Id | null } | { kind: 'tag'; id: Id } | { kind: 'trash' }

export interface NewNote {
  folderId?: Id | null
  title?: string
  body?: string
  tags?: string[]
  pinned?: boolean
}

export interface NoteChange {
  title?: string
  body?: string
  /** The title the edit started from: a change made elsewhere since is merged, not replaced. */
  baseTitle?: string
  /** The body the edit started from, as `baseTitle`. */
  baseBody?: string
}

export interface VaultStatus {
  exists: boolean
  encrypted: boolean
  unlocked: boolean
  /** How the vault opens day to day: Windows Hello or a fingerprint, a PIN,
   * or the password. Old vaults: `plain` (not encrypted) or `device` (opened
   * without asking), both given a password once open. */
  way: VaultWay
  /** A password opens it: every vault but the old ones. */
  hasPassword: boolean
  /** Windows Hello or the platform equivalent can verify the user. */
  biometricAvailable: boolean
  /** This device can keep a key (its key store works): a PIN needs it. */
  deviceAvailable: boolean
  /** A recovery key can open the vault. */
  recovery: boolean
  /** Wrong PINs left before the password is needed, with the PIN way. */
  pinTriesLeft: number | null
}

export type VaultWay = 'plain' | 'device' | 'guarded' | 'pin' | 'password'

/** Digits in a PIN, as the core wants them. */
export const PIN_LENGTH = 6

/** A newer version of the app, downloaded and waiting to be installed. */
export interface AppUpdate {
  version: string
  /** What changed, from the release. */
  notes: string
  /** The portable exe: not downloaded, this page has it. */
  page?: string
  /** It fixes a critical security problem: asked in the middle of the
   * screen. Never installed without the person's yes. */
  critical?: boolean
}

export interface ExportSummary {
  notes: number
  folders: number
  attachments: number
  missingAttachments: string[]
}

/** What a backup is and holds, see crates/core/src/backup.rs. */
export interface BackupManifest {
  format: number
  app: string
  appVersion: string
  createdAt: number
  encrypted: boolean
  notes: number
  folders: number
  versions: number
  files: number
  /** Files whose content the device that made it did not have. */
  filesMissing: number
}

export interface RestoreReport {
  notes: number
  folders: number
  versions: number
  files: number
  /** Notes already in the vault, left as they are. */
  skipped: number
  filesMissing: number
}

/** Where backups go, see crates/core/src/repo/settings.rs. Secrets come
 * back blank: sent blank, the ones set before stay. */
export type BackupTarget =
  | { kind: 'folder'; path: string }
  | {
      kind: 's3'
      endpoint: string
      region: string
      bucket: string
      prefix: string
      accessKey: string
      secretKey: string
      pathStyle: boolean
    }
  | { kind: 'webdav'; url: string; username: string; password: string }

export type BackupSchedule = 'off' | 'hourly' | 'daily' | 'weekly'

/** What to keep: the newest of each period. */
export interface BackupPolicy {
  last: number
  daily: number
  weekly: number
  monthly: number
  yearly: number
}

export interface BackupRunReport {
  snapshot: string
  notes: number
  files: number
  size: number
  stats: {
    newObjects: number
    newBytes: number
    storedBytes: number
    knownObjects: number
    readBytes: number
    packs: number
  }
}

export interface BackupStatus {
  lastRunAt: number | null
  lastSuccessAt: number | null
  lastError: string | null
  /** Its kind, to say it in the person's language. */
  lastErrorKind: string | null
  lastReport: BackupRunReport | null
  snapshots: number
  storedBytes: number
}

/** This device's backups, see commands/backups.rs. */
export interface BackupsView {
  config: {
    target: BackupTarget
    schedule: BackupSchedule
    policy: BackupPolicy
    host: string
  } | null
  status: BackupStatus
  running: boolean
  kinds: BackupTarget['kind'][]
}

export interface BackupSnapshot {
  id: string
  time: number
  host: string
  appVersion: string
  counts: Record<string, number>
  size: number
}

export interface BackupTestRestore {
  snapshot: string
  restored: RestoreReport
  problems: string[]
}

export interface BackupCheck {
  snapshots: number
  packs: number
  objects: number
  storedBytes: number
  problems: string[]
}

/** An export the app opened, see commands/import.rs. */
export interface ImportOpened {
  id: number
  name: string
  entries: { path: string; size: number; modified: number | null }[]
}

/** A file of an imported note: an entry of the export, or stored bytes. */
export interface ImportFileInput {
  name: string
  mime: string
  entry?: string
  sha256?: string
  size?: number
}

export interface ImportNoteInput {
  key: string
  folder: string | null
  title: string
  body: string
  tags: string[]
  created: number | null
  updated: number | null
  pinned: boolean
  trashed: boolean
  sources: ImportFileInput[]
}

export interface ImportBatchInput {
  importer: string
  folders: { key: string; parent: string | null; name: string }[]
  notes: ImportNoteInput[]
}

export interface ImportWritten {
  /** Each note's key with the id it got. */
  notes: [string, Id][]
  /** Files left out: `missing`, `tooLarge`, `typeRefused`, `tooManyFiles`, `unreadable`. */
  leftOut: { note: string; name: string; why: string }[]
}

/** Error sent by the native side. `kind` maps to a translated message. */
export interface BackendError {
  kind: string
  message: string
}

export const MATCH_START = '\u0002'
export const MATCH_END = '\u0003'

/** Which themes the apps may use besides the built-in ones, see
 * crates/protocol/src/extensions.rs. */
export type ThemeRule = 'any' | 'instance'

export interface ExtensionRules {
  themes: ThemeRule
}

export interface OfferedTheme {
  id: string
  name: string
  size: number
  sha256: string
  preinstall: boolean
  uploadedAt: number
}

/** `GET /admin/extensions`, and what the apps read at `/sync/extensions`. */
export interface Extensions {
  rules: ExtensionRules
  themes: OfferedTheme[]
}

/** One theme of the official catalog, see crates/protocol/src/catalog.rs. */
export interface CatalogEntry {
  kind: 'theme'
  id: string
  name: string
  version: string
  description?: string
  author?: string
  file: string
  size: number
  fileSha256: string
  /** What is installed: the theme file's own hash. */
  contentSha256: string
  signature: string
}

export interface Catalog {
  format: number
  generatedAt: number
  entries: CatalogEntry[]
}

/** Where this device's feedback goes, see src-tauri/src/sync/feedback.rs. */
export interface FeedbackWhere {
  available: boolean
  url: string
  host: string
  /** `main`, `server`, or why there is no place: `off`, `none`. */
  via: 'main' | 'server' | 'off' | 'none'
  /** What the report says of this app. */
  version: string
  platform: string
}

/** A report as the form sends it, images as base64. */
export interface FeedbackDraft {
  install: string
  /** What it is about, in one line. */
  about: string
  message: string
  /** An email address or a Discord name. */
  contact?: string
  logs?: string
  images: string[]
  /** SHA-256 hex of a secret kept here, to ask later how the report stands. */
  receipt?: string
}

export interface FeedbackSent {
  host: string
  /** The instance it went to. */
  url: string
  bits: number
  workMs: number
}

/** How a sent report stands. Spam reads as received. */
export interface FeedbackAnswer {
  secret: string
  state: 'received' | 'open' | 'done'
  /** The one answer from whoever reads the reports, empty until given. */
  answer: string
}

/** The official catalog as this device may see it, every entry checked. */
export interface CatalogState {
  /** This build has a catalog key. */
  available: boolean
  /** Through the server signed in to, straight from the catalog, or not at all. */
  via: 'server' | 'direct' | 'none'
  /** Through a server: its admin turned the catalog on. */
  enabled: boolean
  catalog: Catalog | null
  error: string | null
  /** The catalog's host, named before the person turns it on. */
  host: string
}
