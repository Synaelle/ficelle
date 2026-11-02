// SPDX-License-Identifier: AGPL-3.0-or-later
// Feedback reports in a zip: one report, or the whole inbox with a folder
// for each.

import { api, download } from './api.svelte'
import { zip } from './zip'

/** What a report's zip files need of the full report. */
export interface ZipReport {
  id: string
  createdAt: number
  about: string | null
  category: string
  logs: string | null
  /** Left out of the zip: the sender's contact. */
  contact?: string | null
  images: { position: number; kind: string }[]
}

interface Page {
  reports: { id: string }[]
  next?: string
}

interface ZipFile {
  name: string
  data: Uint8Array<ArrayBuffer>
}

/** Reports fetched at once. */
const PARALLEL = 4

/**
 * A report's files: its details, logs and images, without the sender's
 * contact. `images` follow the report's own, a missing one is skipped.
 */
export async function reportFiles(
  report: ZipReport,
  images: (Blob | undefined)[],
): Promise<ZipFile[]> {
  const text = (s: string) => new TextEncoder().encode(s)
  const details: Partial<ZipReport> = { ...report }
  delete details.contact
  const files = [{ name: 'report.json', data: text(JSON.stringify(details, null, 2)) }]
  if (report.logs) files.push({ name: 'logs.txt', data: text(report.logs) })
  for (const [n, image] of report.images.entries()) {
    const file = images[n]
    if (!file) continue
    const data = new Uint8Array(await file.arrayBuffer())
    files.push({ name: `image-${image.position}.${image.kind}`, data })
  }
  return files
}

/** A report's folder in the big zip: date, short id, what it is about. */
export function reportFolder(report: ZipReport): string {
  const date = new Date(report.createdAt).toISOString().slice(0, 10)
  const slug = (report.about || report.category)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40)
  return [date, report.id.slice(0, 8), slug].filter(Boolean).join('-')
}

/** Hands a zip to the browser as a download. */
export function saveBlob(blob: Blob, name: string) {
  const link = document.createElement('a')
  link.href = URL.createObjectURL(blob)
  link.download = name
  link.click()
  // once the download has taken it
  setTimeout(() => URL.revokeObjectURL(link.href), 10_000)
}

/**
 * Every report matching the filter in one download, a folder for each.
 * Returns how many were zipped.
 */
export async function downloadAll(
  filter: { status?: string; platform?: string; version?: string },
  onProgress?: (done: number, total: number) => void,
): Promise<number> {
  const ids: string[] = []
  let before: string | undefined
  do {
    const params = new URLSearchParams({
      ...(filter.status ? { status: filter.status } : {}),
      ...(filter.platform ? { platform: filter.platform } : {}),
      ...(filter.version ? { version: filter.version } : {}),
      ...(before ? { before } : {}),
    })
    const page = await api<Page>('GET', `/admin/feedback/reports?${params}`)
    ids.push(...page.reports.map((r) => r.id))
    before = page.next
  } while (before)

  const files: ZipFile[] = []
  let done = 0
  onProgress?.(0, ids.length)
  for (let i = 0; i < ids.length; i += PARALLEL) {
    const group = await Promise.all(
      ids.slice(i, i + PARALLEL).map(async (id) => {
        const report = await api<ZipReport>('GET', `/admin/feedback/reports/${id}`)
        const images = await Promise.all(
          report.images.map((image) =>
            download(`/admin/feedback/reports/${id}/images/${image.position}`),
          ),
        )
        const folder = reportFolder(report)
        const inside = await reportFiles(report, images)
        onProgress?.(++done, ids.length)
        return inside.map((file) => ({ ...file, name: `${folder}/${file.name}` }))
      }),
    )
    for (const inside of group) files.push(...inside)
  }
  saveBlob(zip(files), `feedback-${new Date().toISOString().slice(0, 10)}.zip`)
  return ids.length
}
