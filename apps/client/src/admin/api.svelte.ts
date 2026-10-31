// SPDX-License-Identifier: AGPL-3.0-or-later
// The admin page talks to the server it is served from. The session lives
// in sessionStorage: it ends with the tab, an admin token should not outlive
// the visit. An expired access token is refreshed once, then the page goes
// back to sign-in.

/** The path the server serves this page under, with its last slash: `/`,
 * or `/notes/` under `server.base_path`. The server says it in `<base>`. */
// read once, nothing about it changes while the page is open
// eslint-disable-next-line svelte/prefer-svelte-reactivity
export const base = new URL('.', document.baseURI).pathname

/** The page's own path below `base`, `/admin` or `/invite/<token>`. */
export function route(): string {
  const path = location.pathname
  return path.startsWith(base) ? `/${path.slice(base.length)}` : path
}

/** `app_protocol::PROTOCOL_VERSION`, bump both together. */
export const PROTOCOL_VERSION = 1

export type Role = 'admin' | 'user'
export type Registration = 'closed' | 'invite' | 'open'
export type AccountKind = 'standard' | 'e2ee'
/** What sign-up creates. */
export type AccountKinds = AccountKind | 'choice'

/** The account master key wrapped twice, opaque to the server. */
export interface KeyBundle {
  version: number
  byPassword: string
  byRecovery: string
}

export interface ServerInfo {
  name: string
  version: string
  minProtocol: number
  maxProtocol: number
}

export interface AuthOptions {
  registration: Registration
  setupNeeded: boolean
  passkeys: boolean
  accountKinds: AccountKinds
  /** A forgotten password can be reset by email. */
  emailReset: boolean
  /** Label of the single sign-on button, absent without it. */
  sso?: string
}

export interface User {
  id: string
  username: string
  displayName: string
  role: Role
  kind: AccountKind
  createdAt: number
  /** An admin made or reset from the command line, which adds a passkey or
   * an authenticator app before the admin pages answer it. */
  factorRequired?: boolean
}

export interface Session {
  accessToken: string
  accessExpiresAt: number
  refreshToken: string
  refreshExpiresAt: number
  deviceId: string
  user: User
}

export interface Overview {
  version: string
  createdAt: string
  users: number
  admins: number
  disabled: number
  groups: number
  devices: number
  registration: Registration
  accountKinds: AccountKinds
  e2ee: number
  coediting: boolean
  /** Admins act as admin only once they have a second step. */
  adminsNeedSecondStep: boolean
  /** The server is for its admin alone ("Sync for myself"). */
  personal: boolean
}

export interface AdminUser {
  id: string
  username: string
  displayName: string
  role: Role
  createdAt: number
  disabledAt: number | null
  lockedUntil: number | null
  lastSeenAt: number | null
  devices: number
  groups: string[]
  secondFactor: boolean
  kind: AccountKind
  email?: string
  deleteAt?: number
}

export interface Device {
  id: string
  name: string
  platform: string
  createdAt: number
  lastSeenAt: number
}

export interface Invite {
  id: string
  token?: string
  link?: string
  createdBy: string
  createdAt: number
  expiresAt: number
  state: 'pending' | 'used' | 'revoked' | 'expired'
}

export interface Group {
  id: string
  name: string
  createdAt: number
  members: number
}

export interface GroupMember {
  userId: string
  username: string
  displayName: string
  addedAt: number
}

export interface AuditEntry {
  id: number
  at: number
  event: string
  userId: string | null
  username: string | null
  deviceId: string | null
  ip: string | null
  detail: string | null
}

export type SecondFactor = 'totp' | 'recoveryCode' | 'passkey' | 'email'

/** The answer of a sign-in whose password was right, when the account has
 * a second factor. */
export interface SecondFactorNeeded {
  challenge: string
  methods: SecondFactor[]
  expiresAt: number
  passkey?: Record<string, unknown>
}

export interface Passkey {
  id: string
  name: string
  createdAt: number
  lastUsedAt: number | null
}

export interface SsoIdentity {
  id: string
  name: string
  createdAt: number
}

export interface Security {
  hasPassword: boolean
  totp: boolean
  recoveryCodesLeft: number
  passkeys: Passkey[]
  sso: SsoIdentity[]
  /** The confirmed email. */
  email?: string
  emailAvailable: boolean
  /** Codes by email are the account's second step. */
  emailCodes: boolean
  /** Set while the account waits for its deletion. */
  deleteAt?: number
}

/** Facts for the privacy page, from the server's settings. */
export interface PrivacyInfo {
  operator?: string
  contact?: string
  notice?: string
  accountKinds: AccountKinds
  email: boolean
  sso?: string
  auditDays: number
  deletionGraceDays: number
  sessionDays: number
  /** Feedback: taken here, kept so long, where this server's people send theirs. */
  feedbackReceive: boolean
  feedbackKeepDays: number
  feedbackSendTo: 'main' | 'here' | 'off'
  feedbackMain: string
}

export interface TotpSetup {
  secret: string
  uri: string
  qrSvg: string
}

export interface PasskeyCeremony {
  id: string
  options: Record<string, unknown>
}

/** A refused call, `code` is one of `app_protocol::error_code`. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly retryAfter?: number,
  ) {
    super(message)
  }
}

const KEY = 'admin-session'
/** This browser's last sign-in, kept past the tab (localStorage): the next
 * sign-in replaces it, so closing the tab leaves no device behind. */
const DEVICE_KEY = 'admin-device'

function load(): Session | null {
  try {
    const raw = sessionStorage.getItem(KEY)
    return raw ? (JSON.parse(raw) as Session) : null
  } catch {
    return null
  }
}

/** Messages for a section the page opens on, after a trip elsewhere. */
export const flash = $state({ account: '' })

/** The signed in admin, null when signed out. */
export const auth: { session: Session | null } = $state({ session: load() })

export function keep(session: Session | null) {
  auth.session = session
  try {
    if (session) sessionStorage.setItem(KEY, JSON.stringify(session))
    else sessionStorage.removeItem(KEY)
    if (session) localStorage.setItem(DEVICE_KEY, session.deviceId)
  } catch {
    // private windows may refuse storage, the session lasts until a reload
  }
}

async function send(
  method: string,
  path: string,
  body?: unknown,
  token?: string,
): Promise<Response> {
  const headers: Record<string, string> = { 'x-protocol-version': String(PROTOCOL_VERSION) }
  // a file (a theme) goes as it is, anything else as json
  const file = body instanceof Blob
  if (file) headers['content-type'] = body.type || 'application/octet-stream'
  else if (body !== undefined) headers['content-type'] = 'application/json'
  if (token) headers.authorization = `Bearer ${token}`
  try {
    return await fetch(`${base}api/v1${path}`, {
      method,
      headers,
      body: file ? body : body === undefined ? undefined : JSON.stringify(body),
    })
  } catch {
    throw new ApiError(0, 'offline', 'the server cannot be reached')
  }
}

async function code(response: Response): Promise<string | undefined> {
  const data: unknown = await response
    .clone()
    .json()
    .catch(() => null)
  return (data as { error?: string } | null)?.error
}

async function read<T>(response: Response): Promise<T> {
  if (response.status === 204) return undefined as T
  const data: unknown = await response.json().catch(() => null)
  if (response.ok) return data as T
  const error = (data ?? {}) as { error?: string; message?: string; retryAfterSeconds?: number }
  throw new ApiError(
    response.status,
    error.error ?? 'internal',
    error.message ?? response.statusText,
    error.retryAfterSeconds,
  )
}

/** One api call, signed in when there is a session. */
export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  let response = await send(method, path, body, auth.session?.accessToken)
  // only an expired token is worth a refresh: a wrong password is a 401
  // too, sending it twice would count twice towards the lock
  if (response.status === 401 && auth.session && (await code(response)) === 'unauthorized') {
    const refreshed = await send('POST', '/auth/refresh', {
      refreshToken: auth.session.refreshToken,
    })
    if (!refreshed.ok) {
      keep(null)
      return read<T>(response)
    }
    keep(await read<Session>(refreshed))
    response = await send(method, path, body, auth.session?.accessToken)
  }
  return read<T>(response)
}

/** A file the server hands out as a download, fetched with the session. */
export async function download(path: string): Promise<Blob> {
  const response = await send('GET', path, undefined, auth.session?.accessToken)
  if (!response.ok) await read(response)
  return response.blob()
}

/** Signs this tab out on the server, then forgets the session. */
export async function signOut() {
  await api('POST', '/auth/logout').catch(() => {})
  keep(null)
}

/** This browser's last sign-in, see `DEVICE_KEY`. */
function lastDevice(): string | undefined {
  try {
    return localStorage.getItem(DEVICE_KEY) ?? undefined
  } catch {
    return undefined
  }
}

/** A short device name for the admin page in the device list, and the
 * sign-in it takes the place of. */
export function thisDevice() {
  const ua = navigator.userAgent
  const browser = /firefox/i.test(ua)
    ? 'Firefox'
    : /edg\//i.test(ua)
      ? 'Edge'
      : /chrome/i.test(ua)
        ? 'Chrome'
        : /safari/i.test(ua)
          ? 'Safari'
          : 'Browser'
  return { name: `${browser}, admin page`, platform: 'web', replaces: lastDevice() }
}

/** A files policy as set at one level, a key left out is inherited. */
export interface PolicySettings {
  maxFileMb?: number
  quotaMb?: number
  allowedTypes?: string[]
  blockedTypes?: string[]
  maxFilesPerNote?: number
  imageMaxPx?: number
  imageQuality?: number
  thumbnails?: boolean
  retentionDays?: number
}

/** The rules in effect, sizes in bytes, 0 for no limit. */
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

export interface PolicySource {
  level: 'user' | 'group' | 'instance' | 'config'
  groups?: string[]
}

export interface PolicyView {
  settings: PolicySettings
  effective: FilesPolicy
  sources: Record<string, PolicySource>
}

export interface PersonStorage {
  userId: string
  username: string
  displayName: string
  usedBytes: number
  fileBytes: number
  files: number
  largestBytes: number
  quotaBytes: number
}

export interface StorageOverview {
  storedBytes: number
  files: number
  referencedBytes: number
  uniqueBytes: number
  noteBytes: number
  people: { total: number; items: PersonStorage[] }
}

export type {
  ThemeRule,
  ExtensionRules,
  OfferedTheme,
  Extensions,
  Catalog,
} from '../lib/backend/types'

/** `GET /admin/catalog`, see crates/protocol/src/catalog.rs. */
export interface CatalogView {
  enabled: boolean
  catalog?: import('../lib/backend/types').Catalog
  error?: string
  fetchedAt?: number
}
