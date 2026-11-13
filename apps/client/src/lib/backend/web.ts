// SPDX-License-Identifier: AGPL-3.0-or-later
// The web version's backend: the apps' commands, answered by the notes core
// in a worker (core.worker.ts) in place of the native side. What only an app
// can do (update itself, keep a key on the device, reach the system's files
// and windows) is left out here, or done the browser's way.

import { brand } from '@app/brand'
import type { invoke } from '@tauri-apps/api/core'
import type { Backend } from './backend'
import type { CoreAnswer, CoreRequest } from './core.worker'
import { pickedFile } from '../pick'
import { backupMade, notesMade } from '../webBackups'
import { createTauriBackend } from './tauri'
import type { Attachment, AttachmentFile } from './types'

const unsupported = (message: string) => ({ kind: 'unsupported', message })

/** Hands a made file to the browser to save, named after `path`, see
 * pickToSave. */
function download(bytes: Uint8Array<ArrayBuffer>, path: string) {
  const url = URL.createObjectURL(new Blob([bytes]))
  const link = document.createElement('a')
  link.href = url
  link.download = path.slice(path.lastIndexOf('/') + 1)
  link.click()
  // the download has its copy by then
  setTimeout(() => URL.revokeObjectURL(url), 60_000)
}

/** A file picked with pickToOpen, whole. */
async function pickedBytes(path: string): Promise<Uint8Array<ArrayBuffer>> {
  const file = pickedFile(path)
  if (!file) throw { kind: 'notFound', message: 'the picked file is gone' }
  return new Uint8Array(await file.arrayBuffer())
}

/** Held by the one tab whose worker has the notes: the browser lets one
 * page at a time keep the storage the core writes to. */
const LOCK = 'notes'

export function createWebBackend(): Backend {
  const waiting = new Map<number, { resolve(value: unknown): void; reject(err: unknown): void }>()
  let next = 0
  let worker: Promise<Worker> | null = null
  /** Asked once already: the next try takes the notes from the other tab. */
  let refused = false
  /** Ends the worker and lets go of the notes. */
  let stop: (() => void) | null = null
  const tabs = new BroadcastChannel(LOCK)

  /** The worker, once this tab holds the notes. The first try only takes
   * them when free; after the "already open" screen, a try asks the tab
   * that has them to let go and waits its turn. */
  function start(): Promise<Worker> {
    const takeOver = refused
    return new Promise<Worker>((resolve, reject) => {
      if (takeOver) tabs.postMessage('take')
      void navigator.locks.request(LOCK, takeOver ? {} : { ifAvailable: true }, (lock) => {
        if (!lock) {
          refused = true
          reject({ kind: 'openElsewhere', message: 'the notes are open in another tab' })
          return
        }
        const started = new Worker(new URL('./core.worker.ts', import.meta.url), {
          type: 'module',
        })
        started.onmessage = ({ data }: MessageEvent<CoreAnswer>) => {
          const pending = waiting.get(data.id)
          waiting.delete(data.id)
          if ('error' in data) pending?.reject(data.error)
          else pending?.resolve(data.value)
        }
        resolve(started)
        // held until another tab takes the notes: this one then stops its
        // worker, which frees the storage, and starts over to say so. Or
        // until the notes are locked, see stop
        return new Promise<void>((release) => {
          stop = () => {
            started.terminate()
            worker = null
            release()
          }
          tabs.onmessage = ({ data }) => {
            if (data !== 'take') return
            stop?.()
            location.reload()
          }
        })
      })
    }).catch((err: unknown) => {
      worker = null
      throw err
    })
  }

  async function send<T>(
    name: string,
    args: Record<string, unknown> = {},
    more: Pick<CoreRequest, 'bytes' | 'binary'> = {},
  ): Promise<T> {
    worker ??= start()
    const ready = await worker
    const id = next++
    return new Promise<T>((resolve, reject) => {
      waiting.set(id, { resolve: resolve as (value: unknown) => void, reject })
      const request: CoreRequest = { id, name, args: JSON.stringify(args), ...more }
      ready.postMessage(request, more.bytes ? [more.bytes.buffer] : [])
    })
  }

  const call = ((name: string, args?: Record<string, unknown>) => send(name, args)) as typeof invoke
  const commands = createTauriBackend(call)

  /** Pictures of the open notes as blob urls, by hash: `fileUrl` answers at
   * once, so they are read when the note's files are listed. */
  const urls = new Map<string, string>()
  async function withUrls(files: AttachmentFile[]): Promise<AttachmentFile[]> {
    for (const file of files) {
      const sha = file.here || !file.thumbSha256 ? file.sha256 : file.thumbSha256
      if (!file.mimeType.startsWith('image/') || urls.has(sha)) continue
      const bytes = await send<Uint8Array<ArrayBuffer>>(
        'read_attachment',
        { sha256: sha },
        { binary: true },
      )
      urls.set(sha, URL.createObjectURL(new Blob([bytes], { type: file.mimeType })))
    }
    return files
  }
  function forgetUrls() {
    for (const url of urls.values()) URL.revokeObjectURL(url)
    urls.clear()
  }

  return {
    ...commands,
    native: false,

    vaultUnlockPin: async () => {
      throw unsupported('a PIN needs the app')
    },
    vaultUnlockBiometric: async () => {
      throw unsupported('biometric unlock needs the app')
    },
    vaultSetWay: async () => {
      throw unsupported('unlock ways need the app')
    },
    vaultCreate: async (way, password, pin) => {
      await commands.vaultCreate(way, password, pin)
      notesMade()
    },
    // the worker ends: nothing of the open notes stays in its memory, and
    // the next call starts a fresh one that has to unlock again
    vaultLock: async () => {
      forgetUrls()
      await commands.vaultLock()
      stop?.()
      stop = null
    },

    appUpdateCheck: async () => null,
    appUpdateLatest: async () => null,
    appUpdatePageOnly: async () => false,
    appUpdateInstall: async () => {
      throw unsupported('updates need the app')
    },
    notificationsAllowed: async () => false,
    notify: async () => {},
    quickWindowOpen: async () => {
      throw unsupported('the quick note needs the desktop app')
    },
    quickWindowReady: async () => {},
    // fonts come from the project's own mirror, as the apps get them, and the
    // browser's cache keeps them
    fontDownload: async (_id, files) => files ?? ['latin-400-normal', 'latin-700-normal'],
    fontFiles: async () => [],
    fontFile: async (id, name) => {
      const response = await fetch(`${brand.fontsUrl}/${id}/${name}.woff2`)
      if (!response.ok) throw { kind: 'notFound', message: `${id} has no ${name}` }
      return response.arrayBuffer()
    },
    systemClock24h: async () => null,
    // the page's own paste and drop events are all the browser has
    clipboardImages: async () => [],
    clipboardImage: async () => new ArrayBuffer(0),
    droppedImages: async () => [],
    droppedImage: async () => new ArrayBuffer(0),

    addAttachment: async (note, file) => {
      const attachment = await send<Attachment>(
        'add_attachment',
        {
          note,
          name: file.name,
          mime: file.type || 'application/octet-stream',
        },
        { bytes: new Uint8Array(await file.arrayBuffer()) },
      )
      // the editor shows a picture as soon as it is attached
      if (attachment.mimeType.startsWith('image/') && !urls.has(attachment.sha256))
        urls.set(attachment.sha256, URL.createObjectURL(file))
      return attachment
    },
    attachInboxFile: async () => {
      throw unsupported('shared files need the app')
    },
    setAttachmentThumb: async (id, thumb) =>
      send('set_attachment_thumb', { id }, { bytes: new Uint8Array(await thumb.arrayBuffer()) }),
    attachmentFiles: async (note) => withUrls(await commands.attachmentFiles(note)),
    fileUrl: (sha256) => urls.get(sha256) ?? '',
    readAttachment: async (sha256) => {
      const bytes = await send<Uint8Array>('read_attachment', { sha256 }, { binary: true })
      return bytes.buffer as ArrayBuffer
    },
    // the browser has no folders to write to: one zip, downloaded
    exportMarkdown: async () => {
      throw unsupported('a folder export needs the app')
    },
    exportMarkdownZip: async (path) => {
      download(await send('export_markdown_zip', {}, { binary: true }), path)
      return send('last_report')
    },
    backupWrite: async (path, password) => {
      download(await send('backup_write', { password }, { binary: true }), path)
      backupMade()
      return send('last_report')
    },
    backupEncrypted: async (path) =>
      send('backup_encrypted', {}, { bytes: await pickedBytes(path) }),
    backupInspect: async (path, password) =>
      send('backup_inspect', { password }, { bytes: await pickedBytes(path) }),
    backupRestore: async (path, password) =>
      send('backup_restore', { password }, { bytes: await pickedBytes(path) }),
    importOpen: async (path) =>
      send(
        'import_open',
        { name: pickedFile(path)?.name ?? path },
        {
          bytes: await pickedBytes(path),
        },
      ),
    importRead: (id, path) => send('import_read', { id, path }, { binary: true }),
    // a copy is sent: the page keeps its own
    importPut: (bytes) => send('import_put', {}, { bytes: bytes.slice() }),

    // every file is here: nothing to fetch from a server
    fetchAttachment: async () => {},
    setAutoFetch: async () => {},

    // feedback goes to the feedback service once it is up, never elsewhere
    feedbackWhere: async () => ({
      available: false,
      url: '',
      host: '',
      via: 'off',
      version: __APP_VERSION__,
      platform: 'web',
    }),
    feedbackSend: async () => {
      throw unsupported('feedback is sent from the apps for now')
    },
    feedbackAnswers: async () => ({ reports: [] }),
  }
}
