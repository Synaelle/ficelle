// SPDX-License-Identifier: AGPL-3.0-or-later
// Helpers over a source: paths, text, sources in memory for the browser
// preview and the tests.

import type { ImportSource, Report, SourceEntry } from './types'

const decoder = new TextDecoder('utf-8', { fatal: true })
const lenientDecoder = new TextDecoder('utf-8')
const legacyDecoder = new TextDecoder('windows-1252')
/** Valid UTF-8 characters beyond ASCII for each bad byte sequence, at least,
 * to read a file as damaged UTF-8 rather than as an older encoding. */
const UTF8_PER_BAD = 4

/** Bytes that are no text: a NUL, or many control characters. */
function isBinary(bytes: Uint8Array): boolean {
  const head = bytes.subarray(0, 8192)
  let controls = 0
  for (const byte of head) {
    if (byte === 0) return true
    if (byte < 32 && byte !== 9 && byte !== 10 && byte !== 12 && byte !== 13) controls++
  }
  return controls > head.length / 20
}

/**
 * An entry as text, UTF-8 with a byte order mark dropped. UTF-8 with a few
 * damaged bytes keeps its text, the bad bytes read as U+FFFD and `report`
 * told. Text in an older single byte encoding is read as Windows-1252,
 * bytes that are no text at all are refused.
 */
export async function readText(
  source: ImportSource,
  path: string,
  report?: Report,
): Promise<string> {
  const bytes = await source.read(path)
  if (isBinary(bytes)) throw new Error(`${path} is not text`)
  try {
    return decoder.decode(bytes).replace(/^\uFEFF/, '')
  } catch {
    // in Windows-1252 text nearly every accent is a bad sequence, valid
    // UTF-8 beyond ASCII is then all but absent
    const text = lenientDecoder.decode(bytes)
    const bad = text.match(/\uFFFD/g)?.length ?? 0
    const valid = text.match(/[^\p{ASCII}\uFFFD]/gu)?.length ?? 0
    if (valid < bad * UTF8_PER_BAD) return legacyDecoder.decode(bytes)
    report?.add({ kind: 'damaged', detail: path })
    return text
  }
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
