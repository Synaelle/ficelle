// SPDX-License-Identifier: AGPL-3.0-or-later
// Temporary dev and preview issue reports. Builds the markdown and saves it through
// the dev_report commands, nothing leaves the machine.

import { invoke } from '@tauri-apps/api/core'
import { prefs } from '../lib/prefs.svelte'

/** The phone side, see DevReports.kt; answers are JSON with an error on failure. */
interface AndroidReports {
  folder(): string
  pickFolder(): void
  create(name: string, markdown: string): string
  addImage(report: string, file: string, base64: string): string
  screenshot(): string
}

declare global {
  interface Window {
    AppReports?: AndroidReports
    __appReportFolder?: () => void
  }
}

/** Phones write reports through the system into a folder the person picks. */
export const phoneReports = () => window.AppReports

/** The picked phone folder, empty until one is chosen. */
export const phoneFolder = () => window.AppReports?.folder() ?? ''

/** Opens the phone's folder picker, resolves with the folder after it closes. */
export function pickPhoneFolder(): Promise<string> {
  return new Promise((resolve) => {
    window.__appReportFolder = () => {
      window.__appReportFolder = undefined
      resolve(phoneFolder())
    }
    window.AppReports?.pickFolder()
  })
}

/** Where reports go on a computer, for the settings to show. */
export const desktopFolder = () =>
  invoke<string>('dev_report_folder', { folder: prefs.reportFolder })

/** Elements with this attribute stay out of the screenshot (the report button). */
const NO_SHOT = 'data-no-shot'

/**
 * The app as it shows now, before the report window opens: the webview's own
 * picture on phones, drawn from the page on computers. Null when it failed,
 * the report goes without.
 */
export async function screenshot(): Promise<File | null> {
  try {
    const android = window.AppReports
    if (android) {
      const base64 = android.screenshot()
      if (!base64) return null
      const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0))
      return new File([bytes], 'screenshot.png', { type: 'image/png' })
    }
    // loaded only here, release builds never reach it
    const { domToBlob } = await import('modern-screenshot')
    const blob = await domToBlob(document.documentElement, {
      scale: window.devicePixelRatio,
      width: window.innerWidth,
      height: window.innerHeight,
      filter: (node) => !(node instanceof Element && node.hasAttribute(NO_SHOT)),
    })
    return new File([blob], 'screenshot.png', { type: 'image/png' })
  } catch (err) {
    console.warn('report screenshot failed', err)
    return null
  }
}

async function base64Of(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer())
  let binary = ''
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  }
  return btoa(binary)
}

function phoneAnswer<T>(json: string): T {
  const answer = JSON.parse(json) as T & { error?: string }
  if (answer.error) throw new Error(answer.error)
  return answer
}

export interface ReportInput {
  text: string
  critical: boolean
  images: File[]
  /** Lines describing the app state, written under "Context". */
  context: string[]
  errors: string[]
  now: Date
}

const MAX_ERRORS = 20
const recent: string[] = []

function remember(line: string) {
  recent.push(`${new Date().toLocaleTimeString('en-GB')} ${line}`)
  if (recent.length > MAX_ERRORS) recent.shift()
}

/** Keeps the last errors so a report shows what went wrong before it. */
export function captureErrors() {
  const original = console.error.bind(console)
  console.error = (...args: unknown[]) => {
    remember(args.map((a) => (a instanceof Error ? (a.stack ?? a.message) : String(a))).join(' '))
    original(...args)
  }
  window.addEventListener('error', (e) => remember(`${e.message} at ${e.filename}:${e.lineno}`))
  window.addEventListener('unhandledrejection', (e) => remember(`unhandled: ${String(e.reason)}`))
}

export const recentErrors = () => [...recent]

const pad = (n: number) => String(n).padStart(2, '0')
const stamp = (d: Date) =>
  `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}`

/** Folder name, sortable by date, critical ones visible at a glance. */
export function reportName(input: Pick<ReportInput, 'text' | 'critical' | 'now'>): string {
  const slug = (input.text.trim().split('\n')[0] ?? '')
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+/, '')
    .slice(0, 40)
    .replace(/-+$/, '')
  return [stamp(input.now), input.critical ? 'critical' : '', slug || 'report']
    .filter(Boolean)
    .join('-')
}

const EXTENSIONS: Record<string, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/gif': 'gif',
  'image/webp': 'webp',
}

export const imageName = (file: File, index: number) =>
  `image-${index + 1}.${EXTENSIONS[file.type] ?? 'png'}`

export function reportMarkdown(input: ReportInput): string {
  const title = input.text.trim().split('\n')[0]?.slice(0, 80) || 'Report'
  const lines = [
    `# ${title}`,
    '',
    '- Status: open',
    `- Priority: ${input.critical ? 'critical' : 'normal'}`,
    `- Created: ${input.now.toLocaleString('en-GB')}`,
    ...input.context.map((line) => `- ${line}`),
    '',
    '## Issue',
    '',
    input.text.trim() || '(no text, see images)',
  ]
  if (input.images.length > 0) {
    lines.push('', '## Images', '')
    input.images.forEach((file, i) => lines.push(`![image ${i + 1}](${imageName(file, i)})`))
  }
  if (input.errors.length > 0) {
    lines.push('', '## Recent errors', '', '```', ...input.errors, '```')
  }
  return `${lines.join('\n')}\n`
}

/** Saves the report and returns the folder it went to. */
export async function saveReport(input: ReportInput): Promise<string> {
  const name = reportName(input)
  const markdown = reportMarkdown(input)
  const android = window.AppReports
  if (android) {
    const saved = phoneAnswer<{ name: string; path: string }>(android.create(name, markdown))
    for (const [i, file] of input.images.entries()) {
      phoneAnswer(android.addImage(saved.name, imageName(file, i), await base64Of(file)))
    }
    return saved.path
  }
  const folder = prefs.reportFolder
  const saved = await invoke<{ name: string; path: string }>('dev_report_create', {
    name,
    markdown,
    folder,
  })
  for (const [i, file] of input.images.entries()) {
    const headers: Record<string, string> = {
      'x-report': saved.name,
      'x-file-name': imageName(file, i),
    }
    if (folder) headers['x-folder'] = encodeURIComponent(folder)
    await invoke('dev_report_add_image', new Uint8Array(await file.arrayBuffer()), { headers })
  }
  return saved.path
}
