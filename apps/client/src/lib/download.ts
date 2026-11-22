// SPDX-License-Identifier: AGPL-3.0-or-later
// A file a browser page makes is saved as a download: it has no folders of
// its own to write to (see pickToSave in the web version).

/** Hands the browser `data` to save, named after the last part of `path`. */
export function download(data: BlobPart, path: string, type = 'application/octet-stream') {
  const url = URL.createObjectURL(new Blob([data], { type }))
  const link = document.createElement('a')
  link.href = url
  link.download = path.slice(path.lastIndexOf('/') + 1)
  link.click()
  // the download has its copy by then
  setTimeout(() => URL.revokeObjectURL(url), 60_000)
}

/** Types a browser tab shows without running anything: an html or svg file
 * would run as this page, next to the notes. */
const SHOWN =
  /^(image\/(png|jpeg|gif|webp|avif|bmp)|audio\/.+|video\/.+|text\/plain|application\/pdf)$/

/** Shows `data` in a new tab when the browser shows its type safely, else
 * saves it as a download named after `path`. */
export function showOrDownload(data: BlobPart, path: string, type: string) {
  if (!SHOWN.test(type)) return download(data, path)
  const url = URL.createObjectURL(new Blob([data], { type }))
  window.open(url, '_blank', 'noopener')
  setTimeout(() => URL.revokeObjectURL(url), 60_000)
}
