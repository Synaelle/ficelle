// SPDX-License-Identifier: AGPL-3.0-or-later
// Images made smaller before they are attached, in the page with a canvas:
// no native code, the same on desktop and phones. WebP keeps transparency,
// JPEG is the fallback where WebP cannot be written.

/** Longest side of the small preview phones show before the whole image. */
export const THUMB_PX = 320
/** Below this an image is its own preview. */
const THUMB_WORTH = 48 * 1024

/** Formats that stay as they are: moving (GIF) or not pixels (SVG). */
const KEEP = ['image/gif', 'image/svg+xml']

async function bitmap(file: Blob): Promise<ImageBitmap | null> {
  try {
    return await createImageBitmap(file)
  } catch {
    return null
  }
}

/** The image drawn at most `maxPx` on its longest side, WebP or JPEG. */
async function encode(image: ImageBitmap, maxPx: number, quality: number): Promise<Blob | null> {
  const scale = Math.min(1, maxPx / Math.max(image.width, image.height))
  const width = Math.max(1, Math.round(image.width * scale))
  const height = Math.max(1, Math.round(image.height * scale))
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const context = canvas.getContext('2d')
  if (!context) return null
  context.imageSmoothingQuality = 'high'
  context.drawImage(image, 0, 0, width, height)
  const as = (type: string) =>
    new Promise<Blob | null>((done) => canvas.toBlob(done, type, quality))
  const webp = await as('image/webp')
  // a browser that cannot write WebP hands back PNG instead
  if (webp?.type === 'image/webp') return webp
  return as('image/jpeg')
}

/** Whether a file is an image the canvas can redraw. */
export function redrawable(file: Blob): boolean {
  return file.type.startsWith('image/') && !KEEP.includes(file.type)
}

/**
 * The image shrunk to `maxPx` on its longest side (0 keeps it as it is).
 * The file comes back unchanged when it is not larger, not an image the
 * canvas can draw, or would not get smaller.
 */
export async function shrinkImage(file: File, maxPx: number, quality: number): Promise<File> {
  if (maxPx <= 0 || !redrawable(file)) return file
  const image = await bitmap(file)
  if (!image || Math.max(image.width, image.height) <= maxPx) return file
  const blob = await encode(image, maxPx, quality)
  image.close()
  if (!blob || blob.size >= file.size) return file
  const ext = blob.type === 'image/webp' ? '.webp' : '.jpg'
  const name = file.name.replace(/\.[^.]*$/, '') + ext
  return new File([blob], name, { type: blob.type })
}

/** A small preview of an image, `null` when the image is small already. */
export async function thumbnailOf(file: Blob): Promise<Blob | null> {
  if (!redrawable(file) || file.size <= THUMB_WORTH) return null
  const image = await bitmap(file)
  if (!image) return null
  const blob = await encode(image, THUMB_PX, 0.7)
  image.close()
  return blob && blob.size < file.size ? blob : null
}
