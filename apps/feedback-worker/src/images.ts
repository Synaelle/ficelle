// SPDX-License-Identifier: AGPL-3.0-or-later
// Images attached to reports: PNG, JPEG or WebP, known by their first
// bytes, their size read from the header without decoding anything. They
// are only stored and shown in the inbox, never decoded here.

export const MAX_IMAGE_SIDE = 4_096

export type ImageKind = 'png' | 'jpeg' | 'webp'

export interface ImageInfo {
  kind: ImageKind
  width: number
  height: number
}

export const MIME: Record<ImageKind, string> = {
  png: 'image/png',
  jpeg: 'image/jpeg',
  webp: 'image/webp',
}

const PNG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]

function startsWith(bytes: Uint8Array, prefix: number[], at = 0): boolean {
  return prefix.every((b, i) => bytes[at + i] === b)
}

function ascii(bytes: Uint8Array, at: number, text: string): boolean {
  return startsWith(
    bytes,
    Array.from(text, (c) => c.charCodeAt(0)),
    at,
  )
}

function view(bytes: Uint8Array): DataView {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
}

function be16(bytes: Uint8Array, at: number): number | undefined {
  return at + 2 > bytes.length ? undefined : view(bytes).getUint16(at)
}

function le16(bytes: Uint8Array, at: number): number | undefined {
  return at + 2 > bytes.length ? undefined : view(bytes).getUint16(at, true)
}

function le24(bytes: Uint8Array, at: number): number | undefined {
  if (at + 3 > bytes.length) return undefined
  return view(bytes).getUint16(at, true) | (view(bytes).getUint8(at + 2) << 16)
}

function be32(bytes: Uint8Array, at: number): number | undefined {
  return at + 4 > bytes.length ? undefined : view(bytes).getUint32(at)
}

/** The header chunk comes first, its width and height big endian. */
function png(bytes: Uint8Array): [number, number] | undefined {
  if (!ascii(bytes, 12, 'IHDR')) return undefined
  const width = be32(bytes, 16)
  const height = be32(bytes, 20)
  return width === undefined || height === undefined ? undefined : [width, height]
}

/** Segments up to the first start of frame, which holds the size. */
function jpeg(bytes: Uint8Array): [number, number] | undefined {
  let at = 2
  for (;;) {
    if (at >= bytes.length || bytes[at] !== 0xff) return undefined
    // any number of 0xff may pad before a marker
    while (at + 1 < bytes.length && bytes[at + 1] === 0xff) at++
    if (at + 1 >= bytes.length) return undefined
    const marker = bytes[at + 1] ?? 0
    if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) {
      // markers without a length
      at += 2
    } else if (marker === 0xd9 || marker === 0xda) {
      // the image or the scan ends before any frame: broken
      return undefined
    } else if (marker >= 0xc0 && marker <= 0xcf && ![0xc4, 0xc8, 0xcc].includes(marker)) {
      const width = be16(bytes, at + 7)
      const height = be16(bytes, at + 5)
      return width === undefined || height === undefined ? undefined : [width, height]
    } else {
      const length = be16(bytes, at + 2)
      if (length === undefined || length < 2) return undefined
      at += 2 + length
    }
  }
}

/** The first chunk is a lossy frame, a lossless one, or the extended header with the canvas size. */
function webp(bytes: Uint8Array): [number, number] | undefined {
  if (ascii(bytes, 12, 'VP8 ')) {
    if (!startsWith(bytes, [0x9d, 0x01, 0x2a], 23)) return undefined
    const width = le16(bytes, 26)
    const height = le16(bytes, 28)
    return width === undefined || height === undefined
      ? undefined
      : [width & 0x3fff, height & 0x3fff]
  }
  if (ascii(bytes, 12, 'VP8L')) {
    if (bytes[20] !== 0x2f || bytes.length < 25) return undefined
    const bits = view(bytes).getUint32(21, true)
    return [(bits & 0x3fff) + 1, ((bits >>> 14) & 0x3fff) + 1]
  }
  if (ascii(bytes, 12, 'VP8X')) {
    const width = le24(bytes, 24)
    const height = le24(bytes, 27)
    return width === undefined || height === undefined ? undefined : [width + 1, height + 1]
  }
  return undefined
}

/** The image's kind and size, or why it is refused. */
export function checkImage(bytes: Uint8Array): ImageInfo | string {
  let kind: ImageKind
  let size: [number, number] | undefined
  if (startsWith(bytes, PNG)) {
    kind = 'png'
    size = png(bytes)
  } else if (startsWith(bytes, [0xff, 0xd8, 0xff])) {
    kind = 'jpeg'
    size = jpeg(bytes)
  } else if (bytes.length >= 12 && ascii(bytes, 0, 'RIFF') && ascii(bytes, 8, 'WEBP')) {
    kind = 'webp'
    size = webp(bytes)
  } else {
    return 'not a PNG, JPEG or WebP image'
  }
  if (!size) return 'an image whose size cannot be read'
  const [width, height] = size
  if (width === 0 || height === 0) return 'an image without pixels'
  if (width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE) return 'an image over 4096 pixels a side'
  return { kind, width, height }
}
