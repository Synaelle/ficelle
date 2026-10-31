// SPDX-License-Identifier: AGPL-3.0-or-later
// Helpers over a source: paths, text, sources in memory for the browser
// preview and the tests.

import type { ImportSource, SourceEntry } from './types'

const decoder = new TextDecoder('utf-8')

/** An entry as text, UTF-8 with a byte order mark dropped. */
export async function readText(source: ImportSource, path: string): Promise<string> {
  return decoder.decode(await source.read(path)).replace(/^\uFEFF/, '')
}

export const baseName = (path: string) => path.slice(path.lastIndexOf('/') + 1)
export const dirName = (path: string) => path.slice(0, Math.max(0, path.lastIndexOf('/')))
export const extension = (path: string) => /\.([^./]+)$/.exec(path)?.[1]?.toLowerCase() ?? ''
export const withoutExtension = (name: string) => name.replace(/\.[^./]+$/, '')

/**
 * A link's target resolved against the folder of the entry holding it:
 * `../img/a b.png` or `img/a%20b.png`. Null when it leaves the export or is
 * not a relative path (a web address, an anchor).
 */
export function resolvePath(from: string, target: string): string | null {
  if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('#') || target.startsWith('//')) {
    return null
  }
  let path = target.replace(/[?#].*$/, '')
  try {
    path = decodeURIComponent(path)
  } catch {
    // a lone % stays as written
  }
  const parts = path.startsWith('/') ? [] : dirName(from).split('/').filter(Boolean)
  for (const part of path.split('/')) {
    if (part === '' || part === '.') continue
    if (part === '..') {
      if (parts.length === 0) return null
      parts.pop()
    } else parts.push(part)
  }
  return parts.length ? parts.join('/') : null
}

/** Media type from a file name, for the files of a note. */
export function mimeOf(name: string): string {
  const types: Record<string, string> = {
    png: 'image/png',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    gif: 'image/gif',
    webp: 'image/webp',
    svg: 'image/svg+xml',
    bmp: 'image/bmp',
    heic: 'image/heic',
    avif: 'image/avif',
    pdf: 'application/pdf',
    mp3: 'audio/mpeg',
    m4a: 'audio/mp4',
    wav: 'audio/wav',
    ogg: 'audio/ogg',
    mp4: 'video/mp4',
    mov: 'video/quicktime',
    webm: 'video/webm',
    zip: 'application/zip',
    txt: 'text/plain',
    csv: 'text/csv',
    json: 'application/json',
    md: 'text/markdown',
    html: 'text/html',
    doc: 'application/msword',
    docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
    pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  }
  return types[extension(name)] ?? 'application/octet-stream'
}

/** A source from files in memory: the browser preview and the tests. */
export function memorySource(
  name: string,
  files: Record<string, string | Uint8Array>,
  modified: number | null = null,
): ImportSource {
  const encoder = new TextEncoder()
  const bytes = new Map(
    Object.entries(files).map(([path, content]) => [
      path,
      typeof content === 'string' ? encoder.encode(content) : content,
    ]),
  )
  const entries: SourceEntry[] = [...bytes].map(([path, content]) => ({
    path,
    size: content.length,
    modified,
  }))
  return {
    name,
    entries,
    nativeId: null,
    read: async (path) => {
      const found = bytes.get(path)
      if (!found) throw { kind: 'notFound', message: `${path} is not in the export` }
      return found
    },
  }
}
