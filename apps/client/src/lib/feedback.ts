// SPDX-License-Identifier: AGPL-3.0-or-later
// Feedback and bug reports: the recent errors and warnings kept for a
// report, cleaned of what could say something about the person's notes or
// computer, this install's random id, images made to fit the server's caps,
// and the draft kept while the form is open or a send failed. The form is
// settings/FeedbackSettings.svelte, the sending src-tauri/src/sync/feedback.rs.

import { prefs } from './prefs.svelte'

/** Lines kept, the newest. */
export const MAX_LINES = 50
/** As the server's caps (crates/protocol/src/feedback.rs). */
export const MAX_ABOUT = 120
export const MAX_MESSAGE = 5_000
export const MAX_CONTACT = 254
export const MAX_LOGS = 64 * 1024
export const MAX_IMAGES = 3
export const MAX_IMAGE_BYTES = 2 * 1024 * 1024
export const MAX_IMAGE_SIDE = 4_096
/** A screenshot made smaller to fit is made this wide at most. */
const FIT_SIDE = 2_560

const lines: string[] = []
let capturing = false

function keep(level: string, args: unknown[]) {
  const text = args
    .map((a) => (a instanceof Error ? (a.stack ?? a.message) : String(a)))
    .join(' ')
    .slice(0, 500)
  lines.push(`${new Date().toISOString().slice(11, 19)} ${level}: ${text}`)
  if (lines.length > MAX_LINES) lines.shift()
}

/** Keeps the last errors and warnings in memory, never on disk. */
export function captureLogs() {
  if (capturing) return
  capturing = true
  for (const level of ['error', 'warn'] as const) {
    const original = console[level].bind(console)
    console[level] = (...args: unknown[]) => {
      keep(level, args)
      original(...args)
    }
  }
  window.addEventListener('error', (e) =>
    keep('error', [`${e.message} at ${e.filename}:${e.lineno}`]),
  )
  window.addEventListener('unhandledrejection', (e) =>
    keep('error', [`unhandled: ${String(e.reason)}`]),
  )
}

export const recentLogs = () => [...lines]

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** Logs without the home folder's name, email addresses or note titles. */
export function scrub(text: string, titles: string[]): string {
  // a user folder's name may hold spaces ("Camille Durand"): it runs to the
  // next separator, or to the end of the line, more taken out rather than less
  let out = text
    .replace(/[A-Za-z]:[\\/]+Users[\\/]+[^\\/'"\n]+/gi, '~')
    .replace(/\/(home|Users)\/[^/'"\n]+/g, '~')
    .replace(/\/(data\/user\/\d+|data\/data)\/[^/\s'"]+/g, '~')
    .replace(/[\w.+-]+@[\w-]+\.[\w.-]+/g, '[email]')
  // longest first, a title inside another goes with it
  const known = [...new Set(titles.map((t) => t.trim()).filter((t) => t.length >= 3))].sort(
    (a, b) => b.length - a.length,
  )
  for (const title of known) out = out.replace(new RegExp(escape(title), 'gi'), '[title]')
  return out
}

/** The logs a report would carry, cleaned, the newest kept within the cap. */
export function reportLogs(titles: string[]): string {
  let text = scrub(recentLogs().join('\n'), titles)
  while (new TextEncoder().encode(text).length > MAX_LOGS) text = text.slice(text.indexOf('\n') + 1)
  return text
}

/** This install's id: random, made once, tied to nothing. */
export function installId(): string {
  if (!/^[0-9a-f]{32}$/.test(prefs.feedbackInstall)) {
    const bytes = crypto.getRandomValues(new Uint8Array(16))
    prefs.feedbackInstall = [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('')
  }
  return prefs.feedbackInstall
}

export interface PreparedImage {
  /** Standard base64, as the server takes it. */
  data: string
  /** For the preview. */
  url: string
  type: string
  width: number
  height: number
  size: number
}

const KINDS = ['image/png', 'image/jpeg', 'image/webp']

function base64(bytes: Uint8Array): string {
  let binary = ''
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  }
  return btoa(binary)
}

async function encode(canvas: HTMLCanvasElement, quality: number): Promise<Blob> {
  return new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('encode'))), 'image/jpeg', quality),
  )
}

/** An image as the server takes it: kept as it is when it fits, otherwise
 * drawn smaller as a JPEG until it does. `null` for a file that is not an
 * image. */
export async function prepareImage(file: Blob): Promise<PreparedImage | null> {
  let bitmap: ImageBitmap
  try {
    bitmap = await createImageBitmap(file)
  } catch {
    return null
  }
  const { width, height } = bitmap
  let blob = file
  let size = { width, height }
  const fits =
    KINDS.includes(file.type) &&
    file.size <= MAX_IMAGE_BYTES &&
    Math.max(width, height) <= MAX_IMAGE_SIDE
  if (!fits) {
    const scale = Math.min(1, FIT_SIDE / Math.max(width, height))
    const canvas = document.createElement('canvas')
    canvas.width = Math.max(1, Math.round(width * scale))
    canvas.height = Math.max(1, Math.round(height * scale))
    const context = canvas.getContext('2d')
    if (!context) return null
    // JPEG has no transparency: what was clear shows white, not black
    context.fillStyle = '#fff'
    context.fillRect(0, 0, canvas.width, canvas.height)
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height)
    size = { width: canvas.width, height: canvas.height }
    for (const quality of [0.9, 0.8, 0.7, 0.6, 0.5, 0.4]) {
      blob = await encode(canvas, quality)
      if (blob.size <= MAX_IMAGE_BYTES) break
    }
    if (blob.size > MAX_IMAGE_BYTES) return null
  }
  bitmap.close()
  const bytes = new Uint8Array(await blob.arrayBuffer())
  return {
    data: base64(bytes),
    url: URL.createObjectURL(blob),
    type: blob.type,
    ...size,
    size: bytes.length,
  }
}

/** Whether the server would take this as a contact: an email address, or a
 * Discord name (today's, an `@` before it allowed, or an older one with its
 * `#1234`). As `contact` in apps/server/src/feedback/mod.rs. */
export function contactOk(text: string): boolean {
  const value = text.trim().toLowerCase()
  if (!value) return true
  if (value.length > MAX_CONTACT || /\s/.test(value)) return false
  const name = value.startsWith('@') ? value.slice(1) : value
  if (name.includes('@')) {
    if (name !== value) return false
    const at = name.indexOf('@')
    const domain = name.slice(at + 1)
    return at > 0 && domain.includes('.') && !domain.includes('@')
  }
  if (/^[a-z0-9_.]{2,32}$/.test(name)) return true
  const hash = name.lastIndexOf('#')
  const user = name.slice(0, hash)
  return (
    hash > 0 &&
    /^\d{4}$/.test(name.slice(hash + 1)) &&
    [...user].length >= 2 &&
    [...user].length <= 32 &&
    !/[@#:]/.test(user)
  )
}

export interface Draft {
  about: string
  message: string
  contact: string
  logs: boolean
}

const DRAFT_KEY = 'feedback-draft'

export const EMPTY_DRAFT: Draft = { about: '', message: '', contact: '', logs: true }

/** The words of the last report not sent, images left out (too large to keep). */
export function loadDraft(): Draft {
  try {
    const raw = localStorage.getItem(DRAFT_KEY)
    if (!raw) return { ...EMPTY_DRAFT }
    const kept = JSON.parse(raw) as Partial<Record<keyof Draft, unknown>>
    const text = (value: unknown) => (typeof value === 'string' ? value : '')
    return {
      about: text(kept.about),
      message: text(kept.message),
      contact: text(kept.contact),
      logs: typeof kept.logs === 'boolean' ? kept.logs : true,
    }
  } catch {
    return { ...EMPTY_DRAFT }
  }
}

export function saveDraft(draft: Draft | null) {
  try {
    if (draft && (draft.about.trim() || draft.message.trim() || draft.contact.trim())) {
      localStorage.setItem(DRAFT_KEY, JSON.stringify(draft))
    } else localStorage.removeItem(DRAFT_KEY)
  } catch {
    // storage may be refused, the draft lasts while the form is open
  }
}

/** A report this app sent, kept here to ask later how it stands. */
export interface SentReport {
  /** Random, never sent: the server holds only its hash, the receipt. */
  secret: string
  /** The instance it went to. */
  url: string
  host: string
  about: string
  /** Milliseconds since 1970. */
  sentAt: number
}

const SENT_KEY = 'feedback-sent'

/** The most reports remembered, the oldest forgotten first. */
export const MAX_SENT = 50

/** A fresh secret and its receipt, the SHA-256 the server keeps. */
export async function newReceipt(): Promise<{ secret: string; receipt: string }> {
  const hex = (bytes: Uint8Array) => [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('')
  const secret = hex(crypto.getRandomValues(new Uint8Array(32)))
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(secret))
  return { secret, receipt: hex(new Uint8Array(digest)) }
}

/** The reports sent from here, newest first. */
export function loadSent(): SentReport[] {
  try {
    const kept: unknown = JSON.parse(localStorage.getItem(SENT_KEY) ?? '[]')
    if (!Array.isArray(kept)) return []
    return kept.filter(
      (r): r is SentReport =>
        typeof r === 'object' &&
        r !== null &&
        typeof r.secret === 'string' &&
        typeof r.url === 'string' &&
        typeof r.host === 'string' &&
        typeof r.about === 'string' &&
        typeof r.sentAt === 'number',
    )
  } catch {
    return []
  }
}

function saveSent(list: SentReport[]) {
  try {
    if (list.length) localStorage.setItem(SENT_KEY, JSON.stringify(list.slice(0, MAX_SENT)))
    else localStorage.removeItem(SENT_KEY)
  } catch {
    // storage may be refused, the report is sent all the same
  }
}

export function rememberSent(report: SentReport) {
  saveSent([report, ...loadSent()])
}

export function forgetSent(secret: string) {
  saveSent(loadSent().filter((r) => r.secret !== secret))
}
