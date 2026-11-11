// SPDX-License-Identifier: AGPL-3.0-or-later
// A report's form and sizes, checked as the server checks them. The
// message of a refusal says what is wrong.

import { checkImage, type ImageInfo } from './images'

export const MAX_ABOUT = 120
export const MAX_MESSAGE = 5_000
export const MAX_LOGS = 64 * 1024
export const MAX_IMAGES = 3
export const MAX_IMAGE_BYTES = 2 * 1024 * 1024
export const MAX_CONTACT = 254

/** Largest report request: three images of 2 MB in base64, the logs and the words. */
export const MAX_REQUEST = 9 * 1024 * 1024

const CATEGORIES = ['bug', 'idea', 'question', 'other'] as const

export interface Incoming {
  /** From apps before 0.2.0. */
  category: string
  /** Trimmed, from apps since 0.2.0. */
  about: string | null
  /** Trimmed, empty when `about` says it all. */
  message: string
  /** An email address or a Discord name, lowercased. */
  contact: string | null
  version: string
  platform: string
  logs: string | null
  install: string | null
  /** The host of the server the app is signed in to, lowercased. */
  server: string | null
  /** 64 lowercase hex, the hash of a secret the app keeps. */
  receipt: string | null
  images: { info: ImageInfo; bytes: Uint8Array }[]
}

export interface Sent {
  challenge: string
  nonce: number
}

const control = /[\p{Cc}]/u
const controlButLines = /[^\P{Cc}\n\r\t]/u
const whitespace = /\p{White_Space}/u
const alphabetic = /\p{Alphabetic}/u
const hex32 = /^[0-9a-f]{32}$/
const hex64 = /^[0-9a-f]{64}$/

const chars = (text: string) => [...text].length

function plain(text: unknown, max: number, allowed: RegExp): text is string {
  return typeof text === 'string' && text !== '' && chars(text) <= max && allowed.test(text)
}

function optional(value: unknown, name: string): string | null {
  if (value === undefined || value === null) return null
  if (typeof value !== 'string') throw new Refusal(`the ${name} is not text`)
  return value
}

class Refusal extends Error {}

/**
 * An email address, or a Discord name: today's (2 to 32 of `a-z 0-9 _ .`,
 * an `@` before it is dropped) or an older one with its `#1234`.
 */
export function contactOf(text: string): string | undefined {
  if (text.length > MAX_CONTACT || [...text].some((c) => whitespace.test(c) || control.test(c))) {
    return undefined
  }
  const lower = text.toLowerCase()
  const name = lower.startsWith('@') ? lower.slice(1) : lower
  const at = name.indexOf('@')
  if (at >= 0) {
    const local = name.slice(0, at)
    const domain = name.slice(at + 1)
    const shaped = local !== '' && domain.includes('.') && !domain.includes('@')
    return shaped && name === lower ? lower : undefined
  }
  const today = name.length >= 2 && name.length <= 32 && /^[a-z0-9_.]+$/.test(name)
  const hash = name.lastIndexOf('#')
  const user = hash >= 0 ? name.slice(0, hash) : ''
  const tag = hash >= 0 ? name.slice(hash + 1) : ''
  const older =
    hash >= 0 && chars(user) >= 2 && chars(user) <= 32 && !/[@#:]/.test(user) && /^\d{4}$/.test(tag)
  return today || older ? name : undefined
}

/** The server a report names: a host name, maybe with a port. */
function serverHost(value: unknown): string | null {
  const server = optional(value, 'server')?.trim().toLowerCase() ?? ''
  if (server === '') return null
  if (server.length > 255 || !/^[a-z0-9.:[\]-]+$/.test(server)) {
    throw new Refusal('the server is not a host name')
  }
  return server
}

function decodeImage(encoded: unknown, n: number): Uint8Array {
  if (typeof encoded !== 'string') throw new Refusal(`image ${n} is not base64`)
  // checked before decoding, a huge string is never decoded
  if (encoded.length > Math.ceil(MAX_IMAGE_BYTES / 3) * 4) {
    throw new Refusal(`image ${n} is over 2 MB`)
  }
  let bytes: Uint8Array
  try {
    bytes = Uint8Array.fromBase64(encoded, { lastChunkHandling: 'strict' })
  } catch {
    throw new Refusal(`image ${n} is not base64`)
  }
  if (bytes.length > MAX_IMAGE_BYTES) throw new Refusal(`image ${n} is over 2 MB`)
  return bytes
}

/** The challenge and its answer, before anything else is looked at. */
export function sentProof(body: unknown): Sent | string {
  if (typeof body !== 'object' || body === null) return 'not a report'
  const { challenge, nonce } = body as Record<string, unknown>
  if (typeof challenge !== 'string' || typeof nonce !== 'number') {
    return 'not a report: a challenge and a nonce are needed'
  }
  return { challenge, nonce }
}

/** Checks a report's form and sizes. */
export function validate(body: Record<string, unknown>): Incoming | string {
  try {
    return check(body)
  } catch (err) {
    if (err instanceof Refusal) return err.message
    throw err
  }
}

function check(body: Record<string, unknown>): Incoming {
  const about = optional(body.about, 'about line')?.trim() || null
  if (about !== null) {
    if (chars(about) > MAX_ABOUT)
      throw new Refusal(`the about line is over ${MAX_ABOUT} characters`)
    if (control.test(about)) throw new Refusal('the about line holds control characters')
  }
  const message = (optional(body.message, 'message') ?? '').trim()
  if (message === '' && about === null) throw new Refusal('the message is empty')
  if (chars(message) > MAX_MESSAGE) {
    throw new Refusal(`the message is over ${MAX_MESSAGE} characters`)
  }
  if (controlButLines.test(message)) throw new Refusal('the message holds control characters')
  if (!plain(body.version, 64, /^[A-Za-z0-9.+\-_ ]+$/)) {
    throw new Refusal('the version is not a version')
  }
  if (!plain(body.platform, 32, /^[a-z0-9\-_]+$/)) {
    throw new Refusal('the platform is not a platform')
  }
  const category = optional(body.category, 'category')
  if (category !== null && !(CATEGORIES as readonly string[]).includes(category)) {
    throw new Refusal('the category is not one of bug, idea, question or other')
  }
  // `email` from apps before 0.2.0
  const contactText = optional(body.contact ?? body.email, 'contact')?.trim() ?? ''
  let contact: string | null = null
  if (contactText !== '') {
    contact = contactOf(contactText) ?? null
    if (contact === null) {
      throw new Refusal('the contact is not an email address or a Discord name')
    }
  }
  let logs = optional(body.logs, 'logs')
  if (logs !== null && logs.trim() === '') logs = null
  if (logs !== null) {
    if (new TextEncoder().encode(logs).length > MAX_LOGS) {
      throw new Refusal(`the logs are over ${MAX_LOGS / 1024} KB`)
    }
    if (logs.includes('\0')) throw new Refusal('the logs hold a NUL')
  }
  const install = optional(body.install, 'install id')
  if (install !== null && !hex32.test(install)) {
    throw new Refusal('the install id is not 32 lowercase hex')
  }
  const server = serverHost(body.server)
  const receipt = optional(body.receipt, 'receipt')
  if (receipt !== null && !hex64.test(receipt)) {
    throw new Refusal('the receipt is not 64 lowercase hex')
  }
  const sent = body.images ?? []
  if (!Array.isArray(sent)) throw new Refusal('the images are not a list')
  if (sent.length > MAX_IMAGES) throw new Refusal(`more than ${MAX_IMAGES} images`)
  const images = sent.map((encoded: unknown, i) => {
    const bytes = decodeImage(encoded, i + 1)
    const info = checkImage(bytes)
    if (typeof info === 'string') throw new Refusal(`image ${i + 1}: ${info}`)
    return { info, bytes }
  })
  return {
    category: category ?? '',
    about,
    message,
    contact,
    version: body.version,
    platform: body.platform,
    logs,
    install,
    server,
    receipt,
    images,
  }
}

/** The spam score and what made it. `repeated` is whether the same words came in the last day. */
export function spamScore(text: string, repeated: boolean): { score: number; flags: string[] } {
  let score = 0
  const flags: string[] = []
  const lower = text.toLowerCase()
  const links = ['http://', 'https://', 'www.'].reduce((n, p) => n + lower.split(p).length - 1, 0)
  if (links >= 3) {
    score += 3
    flags.push('links')
  } else if (links > 0) {
    score += 1
    flags.push('link')
  }
  if (repeated) {
    score += 3
    flags.push('repeated')
  }
  const visible = [...text].filter((c) => !whitespace.test(c))
  const letters = visible.filter((c) => alphabetic.test(c)).length
  if (letters * 2 < visible.length) {
    score += 2
    flags.push('symbols')
  }
  if (chars(text) < 10) {
    score += 2
    flags.push('short')
  }
  return { score, flags }
}

/** A spam score from here files the report as spam. */
export const SPAM_SCORE = 3
