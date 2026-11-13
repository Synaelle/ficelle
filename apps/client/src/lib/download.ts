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
