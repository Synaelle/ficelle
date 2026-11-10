<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Live rendered markdown editor (TipTap). Markdown goes in, markdown comes
     out through onchange, serialized after a short pause so typing in long
     notes stays light. -->
<script lang="ts">
  import { Image as TauriImage } from '@tauri-apps/api/image'
  import { writeImage } from '@tauri-apps/plugin-clipboard-manager'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { toasts } from '../lib/toast.svelte'
  import { headingSlug } from './headingSlug'
  import { Editor } from '@tiptap/core'
  import type { Node as PMNode } from '@tiptap/pm/model'
  import { NodeSelection, TextSelection } from '@tiptap/pm/state'
  import { onDestroy, onMount } from 'svelte'
  import { save as pickSavePath } from '@tauri-apps/plugin-dialog'
  import { ATTACHMENT_PREFIX } from '../lib/attachments'
  import { backend, type AttachmentFile } from '../lib/backend'
  import { shrinkImage, thumbnailOf } from '../lib/images'
  import { FILES_CHANGED } from '../lib/sync.svelte'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { overlay, SEPARATOR, textItems, type MenuItem } from '../lib/overlay.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { prefs, shortcutFor } from '../lib/prefs.svelte'
  import type { ActionId } from '../lib/shortcuts'
  import { isBackendError, workspace } from '../lib/workspace.svelte'
  import { createExtensions } from './extensions'
  import { patchMarkdown, REMOTE } from './patch'
  import { looksLikeMarkdown } from './pasteMarkdown'
  import { ATTACHMENTS_CHANGED, setImageLayout, WRAPS, type Wrap } from './extensions/image'
  import type { SuggestEvent } from './extensions/wikiLink'
  import TableTools from './TableTools.svelte'
  import Toolbar from './Toolbar.svelte'
  import WikiSuggest from './WikiSuggest.svelte'
  import { MOBILE } from '../lib/platform'
  import './editor.css'

  let {
    noteId,
    markdown,
    onchange,
    onopenlink,
    readonly = false,
  }: {
    noteId: string
    markdown: string
    onchange: (markdown: string) => void
    onopenlink: (title: string, heading: string | null) => Promise<boolean>
    /** Shared with this account to read only: shown, never changed. */
    readonly?: boolean
  } = $props()

  /** Idle time before the document is turned back into markdown. */
  const SERIALIZE_DELAY_MS = 250
  /** Phones and tablets, where the system spells and selects. */
  const PHONE = MOBILE
  /** Touch screens have the toolbar fixed over the bottom of the note: the
   * caret is kept above it, not just inside the window. */
  const TOUCH = window.matchMedia('(pointer: coarse)').matches
  const CLEAR_BELOW = TOUCH ? 96 : 5

  /** The keyboard opening shrinks the window: the caret goes back above it. */
  function keepCaret() {
    if (editor?.isFocused) editor.commands.scrollIntoView()
  }

  let element: HTMLDivElement
  let editor: Editor | null = $state(null)
  /** Bumped on every transaction so toolbar states recompute. */
  let tick = $state(0)
  // the editor's own teardown blurs it, a transaction Svelte may not count
  let destroyed = false
  let timer: ReturnType<typeof setTimeout> | undefined
  // a lookup table read by the image view, never rendered, so it need not be reactive
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const urls = new Map<string, string>()
  let suggest: SuggestEvent | null = $state(null)
  let suggestRef: WikiSuggest | undefined = $state()
  let fileInput: HTMLInputElement
  /**
   * Document as opened. Markdown written by other apps (or older versions of
   * this one) can serialize differently without any edit, comparing the
   * documents keeps that from saving and bumping the note's date.
   */
  let opened: PMNode | null = null

  /**
   * A name from a link, as written. A broken escape (`%E0%A4%A`) would
   * throw and leave the note unopened for everyone who reads it: it stays
   * as it is instead.
   */
  function decoded(text: string, decode = decodeURIComponent): string {
    try {
      return decode(text)
    } catch {
      return text
    }
  }

  /** attachments/<name> in markdown, a real url in the webview. */
  function resolveSrc(src: string): string {
    if (!src.startsWith(ATTACHMENT_PREFIX)) return src
    return urls.get(decoded(src.slice(ATTACHMENT_PREFIX.length))) ?? ''
  }

  /** The note's attachments by name, with where each file stands. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const files = new Map<string, AttachmentFile>()
  /** Bumped when files arrive, so an image that failed loads again. */
  let version = 0

  /**
   * An image shows its file when it is here, else its preview while the
   * file is fetched. Other files are fetched when opened.
   */
  function remember(file: AttachmentFile) {
    files.set(file.fileName, file)
    const sha = file.here || !file.thumbSha256 ? file.sha256 : file.thumbSha256
    const url = backend.fileUrl(sha)
    // a blob url (the browser's test backend) takes no query
    urls.set(file.fileName, url.startsWith('blob:') ? url : `${url}?v=${version}`)
    if (!file.here && file.mimeType.startsWith('image/') && !file.refused)
      void backend.fetchAttachment(file.sha256).catch(() => {})
  }

  async function lookAgain() {
    version += 1
    for (const file of await backend.attachmentFiles(noteId)) remember(file)
    window.dispatchEvent(new Event(ATTACHMENTS_CHANGED))
  }

  /** What can be done with a file of the note: save a copy, remove it. */
  function fileItems(file: AttachmentFile): MenuItem[] {
    const items: MenuItem[] = [
      { label: t('files.saveCopy'), icon: 'export', run: () => void saveCopy(file) },
    ]
    // the picture itself on the clipboard; Android's clipboard takes text only
    if (file.mimeType.startsWith('image/') && backend.native && !PHONE)
      items.unshift({ label: t('files.copyImage'), icon: 'copy', run: () => void copyImage(file) })
    if (!readonly)
      items.push({
        label: t('files.remove'),
        icon: 'trash',
        danger: true,
        run: () => void removeFile(file),
      })
    return items
  }

  /** Puts a picture of the note on the clipboard, as an image other apps paste. */
  async function copyImage(file: AttachmentFile) {
    try {
      if (!file.here) await backend.fetchAttachment(file.sha256)
      const bytes = await backend.readAttachment(file.sha256)
      const bitmap = await createImageBitmap(new Blob([bytes], { type: file.mimeType }))
      const canvas = new OffscreenCanvas(bitmap.width, bitmap.height)
      const context = canvas.getContext('2d')
      if (!context) throw new Error('no canvas')
      context.drawImage(bitmap, 0, 0)
      const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data
      const image = await TauriImage.new(new Uint8Array(pixels.buffer), bitmap.width, bitmap.height)
      await writeImage(image)
      toasts.push(t('files.title'), t('files.imageCopied'))
    } catch (err) {
      console.warn('copy image failed', err)
      toasts.push(t('files.title'), t('files.imageNotCopied'))
    }
  }

  /** The menu of a file linked in the text. */
  function fileMenu(event: MouseEvent, file: AttachmentFile) {
    overlay.openMenu(event, fileItems(file))
  }

  /** The file an image of the text shows, if it is one of the note's. */
  function fileOfImage(img: HTMLImageElement): AttachmentFile | undefined {
    const src = img.getAttribute('src') ?? ''
    for (const [name, url] of urls) if (url === src) return files.get(name)
    return undefined
  }

  /**
   * Removes a file from the note: its images and links go from the text,
   * the file is marked removed (kept a while, so other devices and an undo
   * of the text still find it, then deleted).
   */
  export async function removeFile(file: AttachmentFile) {
    const current = editor
    if (!current) return
    const target = ATTACHMENT_PREFIX + encodeURI(file.fileName)
    const points = (src: unknown) =>
      typeof src === 'string' &&
      (src === target || decoded(src, decodeURI) === decoded(target, decodeURI))
    const ranges: [number, number][] = []
    current.state.doc.descendants((node, pos) => {
      if (node.type.name === 'image' && points(node.attrs.src))
        ranges.push([pos, pos + node.nodeSize])
      else if (
        node.isText &&
        node.marks.some((m) => m.type.name === 'link' && points(m.attrs.href))
      )
        ranges.push([pos, pos + node.nodeSize])
    })
    if (ranges.length > 0) {
      const tr = current.state.tr
      for (const [from, to] of ranges.reverse()) tr.delete(from, to)
      current.view.dispatch(tr)
    }
    try {
      await backend.removeAttachment(file.id)
      files.delete(file.fileName)
      save()
    } catch (err) {
      workspace.error = isBackendError(err) ? err.kind : 'unknown'
    }
  }

  async function saveCopy(file: AttachmentFile) {
    try {
      if (!file.here) await backend.fetchAttachment(file.sha256)
      const path = await pickSavePath({ defaultPath: file.fileName })
      if (path) await backend.saveAttachment(file.sha256, path)
    } catch (err) {
      workspace.error = isBackendError(err) ? err.kind : 'unknown'
    }
  }

  /** Editor bindings for formatting actions, from the user's shortcut settings. */
  function keyBindings(): Record<string, () => boolean> {
    const run = (action: ActionId) => {
      const chain = () => editor?.chain().focus()
      const commands: Partial<Record<ActionId, () => boolean>> = {
        bold: () => chain()?.toggleBold().run() ?? false,
        italic: () => chain()?.toggleItalic().run() ?? false,
        strike: () => chain()?.toggleStrike().run() ?? false,
        code: () => chain()?.toggleCode().run() ?? false,
        checklist: () => chain()?.toggleTaskList().run() ?? false,
        link: () => (void promptLink(), true),
      }
      return commands[action]?.() ?? false
    }
    const tiptapKey = (binding: string) =>
      binding
        .split('+')
        .map((part) => (part.length === 1 ? part.toLowerCase() : part))
        .join('-')
    const actions: ActionId[] = ['bold', 'italic', 'strike', 'code', 'checklist', 'link']
    return Object.fromEntries(actions.map((a) => [tiptapKey(shortcutFor(a)), () => run(a)]))
  }

  function scheduleSave() {
    clearTimeout(timer)
    timer = setTimeout(save, SERIALIZE_DELAY_MS)
  }

  /** The markdown the editor shows: what it opened with, sent, or took in. */
  let shown = ''

  function save() {
    clearTimeout(timer)
    if (!editor || (opened && editor.state.doc.eq(opened))) return
    const next = editor.getMarkdown()
    shown = next
    if (next !== markdown) onchange(next)
    // what is saved is the new start: an edit taken back afterwards (a file
    // inserted then removed) differs from it, and is saved too
    opened = editor.state.doc
  }

  // another device's text for this note: patched in, the caret stays
  $effect(() => {
    const incoming = markdown
    if (!editor || incoming === shown) return
    shown = incoming
    if (patchMarkdown(editor, incoming)) opened = editor.state.doc
  })

  /** What a file's mention is in the text: the image itself, or a link to it. */
  function mentionOf(file: { fileName: string; mimeType: string }) {
    const src = ATTACHMENT_PREFIX + encodeURI(file.fileName)
    return file.mimeType.startsWith('image/')
      ? { type: 'image', attrs: { src, alt: file.fileName.replace(/\.[^.]+$/, '') } }
      : { type: 'text', text: file.fileName, marks: [{ type: 'link', attrs: { href: src } }] }
  }

  /** Whether the person placed the caret in this note since it opened. */
  let placed = false

  /** Mentions a file of the note at the caret, or on a line of its own at
   * the end when the caret was never placed (it would sit before the first
   * word otherwise). */
  export function mentionFile(file: AttachmentFile) {
    if (placed) editor?.chain().focus().insertContent(mentionOf(file)).run()
    else
      editor
        ?.chain()
        .focus('end')
        .insertContent({ type: 'paragraph', content: [mentionOf(file)] })
        .run()
  }

  /** Adds files to the note without mentioning them in the text. */
  export async function attachFiles(chosen: File[]) {
    await insertFiles(chosen, undefined, false)
    window.dispatchEvent(new Event(ATTACHMENTS_CHANGED))
  }

  /** The files a paste carries, as files or as file items (WebKitGTK's way). */
  function filesOf(data: DataTransfer | null): File[] {
    const files = [...(data?.files ?? [])]
    if (files.length > 0 || !data) return files
    return [...data.items].flatMap((item) => {
      const file = item.kind === 'file' ? item.getAsFile() : null
      return file ? [file] : []
    })
  }

  /** A paste that is an image the page cannot read itself. */
  function copiedImage(data: DataTransfer, text: string): boolean {
    if (data.types.some((type) => type.startsWith('image/'))) return true
    if (data.types.includes('text/uri-list')) return true
    const lines = text.trim().split('\n')
    return lines.every((line) =>
      /^(file:\/\/|\/)\S.*\.(png|jpe?g|gif|webp|avif|bmp)$/i.test(line.trim()),
    )
  }

  /**
   * Files dropped from a file manager. On Linux the window takes them itself
   * (WebKitGTK gives the page only their paths): the app keeps the dropped
   * images and the page asks for them by number. Elsewhere this never fires
   * and the page's own drop (handleDrop) gets the files.
   */
  async function dropFromSystem(x: number, y: number) {
    if (!editor || readonly) return
    const found = await backend.droppedImages().catch(() => [])
    const files: File[] = []
    for (const [index, image] of found.entries()) {
      const bytes = await backend.droppedImage(index).catch(() => null)
      if (bytes) files.push(new File([bytes], image.name, { type: image.mime }))
    }
    if (files.length === 0) return
    // where the pointer let go, in the note; the end of the cursor otherwise
    const at = editor.view.posAtCoords({ left: x, top: y })?.pos
    await insertFiles(files, at)
  }

  /** The clipboard's images read by the app; its text when it holds none. */
  async function pasteFromSystem(text: string) {
    const found = await backend.clipboardImages().catch(() => [])
    if (found.length === 0) {
      if (text) editor?.chain().focus().insertContent(text).run()
      return
    }
    const files: File[] = []
    for (const [index, image] of found.entries()) {
      const bytes = await backend.clipboardImage(index).catch(() => null)
      if (bytes) files.push(new File([bytes], image.name, { type: image.mime }))
    }
    await insertFiles(files)
  }

  async function insertFiles(chosen: File[], at?: number, mention = true) {
    const policy = (await backend.filesInfo().catch(() => null))?.policy
    for (const picked of chosen) {
      try {
        const file = policy?.imageMaxPx
          ? await shrinkImage(picked, policy.imageMaxPx, policy.imageQuality / 100)
          : picked
        const attachment = await backend.addAttachment(noteId, file)
        // a preview for phones and slow links, only worth it with a server
        const thumb = policy?.thumbnails ? await thumbnailOf(file) : null
        if (thumb) await backend.setAttachmentThumb(attachment.id, thumb)
        remember({
          ...attachment,
          thumbSha256: attachment.thumbSha256,
          here: true,
          refused: null,
        })
        if (!mention) continue
        const content = mentionOf({ fileName: attachment.fileName, mimeType: file.type })
        const chain = editor?.chain().focus()
        if (at !== undefined) chain?.insertContentAt(at, content).run()
        else chain?.insertContent(content).run()
      } catch (err) {
        workspace.error = isBackendError(err) ? err.kind : 'unknown'
      }
    }
  }

  /** Notes for [[query, or headings of a note for [[Note#query. */
  async function suggestLinks(query: string) {
    const hash = query.indexOf('#')
    if (hash < 0) {
      const notes = await backend.linkSuggestions(query, 8)
      return notes.filter((n) => n.id !== noteId).map((n) => ({ title: n.title }))
    }
    const title = query.slice(0, hash).trim()
    const prefix = query
      .slice(hash + 1)
      .trim()
      .toLowerCase()
    const id = title ? await backend.resolveLink(title) : noteId
    if (!id) return []
    const headings = await backend.noteHeadings(id)
    return headings
      .filter((h) => h.toLowerCase().includes(prefix))
      .map((heading) => ({ title, heading }))
  }

  /** Scrolls to a heading by its text, after following a [[Note#Heading]] link. */
  function scrollToHeading(text: string) {
    const wanted = text.trim().toLowerCase()
    const found = [...element.querySelectorAll('h1, h2, h3, h4, h5, h6')].find(
      (h) => h.textContent?.trim().toLowerCase() === wanted,
    )
    found?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  }

  /** The heading whose GitHub anchor this is, scrolled to. */
  function scrollToAnchor(anchor: string) {
    const wanted = anchor.toLowerCase()
    const found = [...element.querySelectorAll('h1, h2, h3, h4, h5, h6')].find(
      (h) => headingSlug(h.textContent ?? '') === wanted,
    )
    found?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  }

  async function promptLink() {
    if (!editor) return
    const current = editor.getAttributes('link').href as string | undefined
    const href = await overlay.askText(t('editor.linkPrompt'), current ?? 'https://')
    if (href === null || !editor) return
    if (href === '') editor.chain().focus().unsetLink().run()
    else editor.chain().focus().extendMarkRange('link').setLink({ href }).run()
  }

  let stopDrops: (() => void) | null = null
  onMount(async () => {
    if (backend.native && !MOBILE)
      void getCurrentWebview()
        .onDragDropEvent(({ payload }) => {
          if (payload.type !== 'drop') return
          // the system gives device pixels, the page's own size setting zooms it
          const scale = window.devicePixelRatio
          void dropFromSystem(payload.position.x / scale, payload.position.y / scale)
        })
        .then((stop) => (destroyed ? stop() : (stopDrops = stop)))
    for (const file of await backend.attachmentFiles(noteId)) remember(file)
    window.addEventListener(FILES_CHANGED, lookAgain)
    if (TOUCH) window.addEventListener('resize', keepCaret)
    editor = new Editor({
      element,
      extensions: createExtensions({
        resolveSrc,
        keys: keyBindings(),
        autoLists: prefs.autoLists,
        emojiShortcodes: prefs.emojiShortcodes,
        arrow: prefs.arrow,
        placeholder: () => t('note.bodyPlaceholder'),
        codeLabels: { picker: () => t('editor.codeLanguage'), plain: () => t('editor.plainText') },
        wiki: {
          suggest: suggestLinks,
          onSuggest: (event) => {
            if (event.phase === 'key') return suggestRef?.key(event.key) ?? false
            suggest = event.phase === 'exit' ? null : event
            return false
          },
        },
      }),
      content: markdown,
      contentType: 'markdown',
      editable: !readonly,
      editorProps: {
        attributes: {
          class: 'prose',
          'aria-label': t('editor.label'),
          // off unless chosen. On a phone, never: a tap on an unknown word
          // opens the system's spelling popup instead of the usual selection
          spellcheck: prefs.spellcheck && !PHONE ? 'true' : 'false',
        },
        scrollMargin: { top: 5, bottom: CLEAR_BELOW, left: 5, right: 5 },
        scrollThreshold: { top: 0, bottom: CLEAR_BELOW, left: 0, right: 0 },
        handlePaste: (view, event) => {
          const data = event.clipboardData
          const files = filesOf(data)
          if (files.length > 0) {
            void insertFiles(files)
            return true
          }
          const text = data?.getData('text/plain') ?? ''
          // a form feed or vertical tab (text copied from a PDF) would be
          // dropped and run pages together: pasted as line breaks instead
          if (/[\f\v]/.test(text)) {
            view.pasteText(text.replace(/[\f\v]/g, '\n'))
            return true
          }
          // a copied image file or picture the page gets only as a path, or not
          // at all: the app reads it from the system clipboard instead
          if (backend.native && data && copiedImage(data, text)) {
            void pasteFromSystem(text)
            return true
          }
          // a copy from a note carries its exact content, and a code block keeps text as is
          if (event.clipboardData?.getData('text/html').includes('data-pm-slice')) return false
          if (view.state.selection.$from.parent.type.spec.code) return false
          if (!editor || !looksLikeMarkdown(text)) return false
          editor.commands.insertContent(text, { contentType: 'markdown' })
          return true
        },
        handleDrop: (view, event) => {
          const files = [...(event.dataTransfer?.files ?? [])]
          if (files.length === 0) return false
          event.preventDefault()
          const at = view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos
          void insertFiles(files, at)
          return true
        },
        handleClick: (_view, _pos, event) => {
          const link = (event.target as HTMLElement | null)?.closest('a')
          const href = link?.getAttribute('href') ?? ''
          // a link to a heading of this note, `#its-anchor` as GitHub writes it
          if (href.startsWith('#') && href.length > 1) {
            event.preventDefault()
            scrollToAnchor(decoded(href.slice(1)))
            return true
          }
          if (!href.startsWith(ATTACHMENT_PREFIX)) return false
          const file = files.get(decoded(href.slice(ATTACHMENT_PREFIX.length)))
          if (!file) return false
          event.preventDefault()
          fileMenu(event, file)
          return true
        },
        handleClickOn: (_view, _pos, node, _nodePos, event) => {
          if (node.type.name !== 'wikiLink') return false
          event.preventDefault()
          save()
          const { target, heading } = node.attrs as { target: string; heading: string | null }
          // a link to a heading of this same note only scrolls
          if (!target && heading) return (scrollToHeading(heading), true)
          void onopenlink(target, heading).then((found) => {
            if (found) return
            void overlay
              .ask(t('editor.createLinked', { title: target }), { confirmLabel: t('menu.create') })
              .then((yes) => {
                if (yes) void workspace.create({ title: target })
              })
          })
          return true
        },
      },
      onUpdate: ({ transaction }) => {
        if (!transaction.getMeta(REMOTE)) scheduleSave()
      },
      onTransaction: ({ transaction }) => {
        if (destroyed) return
        // focus moving elsewhere (a screen drawn in its place focusing its
        // field) blurs the editor while Svelte draws: count it just after
        if (transaction.getMeta('blur') || transaction.getMeta('focus')) {
          queueMicrotask(() => {
            if (!destroyed) tick += 1
          })
        } else tick += 1
      },
      onFocus: () => {
        placed = true
      },
      // a blur can come while Svelte draws another screen: saved just after,
      // and a destroyed editor was saved by its teardown already
      onBlur: () =>
        queueMicrotask(() => {
          if (!destroyed) save()
        }),
    })
    // plugins such as the trailing paragraph adjust the document on its first
    // transaction, run that now so a click is not taken for an edit
    editor.view.dispatch(editor.state.tr.setMeta('addToHistory', false))
    opened = editor.state.doc
    shown = markdown
    if (workspace.pendingHeading) {
      scrollToHeading(workspace.pendingHeading)
      workspace.pendingHeading = null
    }
    if (!readonly) workspace.serializeEditor = save
  })

  /** A right click outside the selection takes the word under the pointer,
   * so bold, copy and the rest act on it; between words, the caret goes
   * there. */
  function selectWordAt(current: Editor, event: MouseEvent) {
    const at = current.view.posAtCoords({ left: event.clientX, top: event.clientY })
    if (!at) return
    const { from, to } = current.state.selection
    if (from !== to && at.pos >= from && at.pos <= to) return
    const spot = current.state.doc.resolve(at.pos)
    const text = spot.parent.textBetween(0, spot.parent.content.size, undefined, ' ')
    const offset = spot.parentOffset
    const word = /[\p{L}\p{N}_'’-]/u
    let start = offset
    let end = offset
    while (start > 0 && word.test(text[start - 1] ?? '')) start -= 1
    while (end < text.length && word.test(text[end] ?? '')) end += 1
    const base = spot.start()
    current.view.dispatch(
      current.state.tr.setSelection(
        TextSelection.create(current.state.doc, base + start, base + end),
      ),
    )
  }

  /** Text editing items plus formatting, in place of the webview menu. */
  function editorMenu(event: MouseEvent) {
    const field = (event.target as HTMLElement).closest<HTMLElement>('[contenteditable=true]')
    const current = editor
    if (!current || !field) return
    // an image of the note: its file's actions first
    const image = event.target instanceof HTMLImageElement ? fileOfImage(event.target) : undefined
    // a long touch on text is the phone's: selection handles, copy and paste
    if (!image && overlay.byTouch) return
    if (!image) selectWordAt(current, event)
    const chain = () => current.chain().focus()
    overlay.openMenu(event, [
      ...(image ? [...fileItems(image), SEPARATOR] : []),
      ...textItems(field),
      SEPARATOR,
      { label: t('editor.bold'), icon: 'bold', run: () => chain().toggleBold().run() },
      { label: t('editor.italic'), icon: 'italic', run: () => chain().toggleItalic().run() },
      { label: t('editor.strike'), icon: 'strike', run: () => chain().toggleStrike().run() },
      { label: t('editor.link'), icon: 'link', run: () => void promptLink() },
    ])
  }

  onDestroy(() => {
    if (workspace.serializeEditor === save) workspace.serializeEditor = null
    save()
    window.removeEventListener(FILES_CHANGED, lookAgain)
    window.removeEventListener('resize', keepCaret)
    destroyed = true
    stopDrops?.()
    editor?.destroy()
  })

  const selectedImage = $derived.by(() => {
    void tick
    const selection = editor?.state.selection
    return selection instanceof NodeSelection && selection.node.type.name === 'image'
      ? selection
      : null
  })

  let imageBar: HTMLDivElement | undefined = $state()
  const imageBarStyle = $derived.by(() => {
    void tick
    if (!selectedImage || !editor) return ''
    const dom = editor.view.nodeDOM(selectedImage.from) as HTMLElement | null
    const box = dom?.getBoundingClientRect()
    const host = element?.getBoundingClientRect()
    if (!box || !host) return ''
    // above the image, or just inside its top edge when the toolbar is in the way
    const top = box.top - host.top - 44
    // kept inside the note on its right too
    const room = host.width - (imageBar?.offsetWidth ?? 0)
    const left = Math.max(0, Math.min(box.left - host.left, room))
    return `top:${top < 0 ? box.top - host.top + 8 : top}px;left:${left}px`
  })

  const WRAP_ICONS: Record<Wrap, IconName> = {
    inline: 'layoutInline',
    left: 'layoutLeft',
    right: 'layoutRight',
    center: 'layoutCenter',
    full: 'layoutFull',
  }
</script>

<div class="editor">
  {#if editor && !readonly}
    <Toolbar {editor} {tick} onlink={promptLink} onimage={() => fileInput.click()} />
  {/if}
  <div class="surface" bind:this={element} oncontextmenu={editorMenu} role="presentation">
    {#if selectedImage && editor}
      <div
        class="image-bar"
        bind:this={imageBar}
        style={imageBarStyle}
        role="toolbar"
        aria-label={t('editor.imageLayout')}
      >
        {#each WRAPS as wrap (wrap)}
          <button
            class="icon-btn"
            aria-label={t(`editor.wrap_${wrap}`)}
            title={t(`editor.wrap_${wrap}`)}
            aria-pressed={selectedImage.node.attrs.wrap === wrap}
            onclick={() => editor && setImageLayout(editor, selectedImage.from, { wrap })}
          >
            <Icon name={WRAP_ICONS[wrap]} size={16} />
          </button>
        {/each}
      </div>
    {/if}
    {#if editor && !readonly}
      <TableTools {editor} {tick} />
    {/if}
  </div>
  {#if suggest && suggest.phase !== 'key' && suggest.phase !== 'exit'}
    <WikiSuggest bind:this={suggestRef} props={suggest.props} />
  {/if}
  <input
    bind:this={fileInput}
    type="file"
    accept="image/*"
    multiple
    hidden
    onchange={(e) => {
      void insertFiles([...(e.currentTarget.files ?? [])])
      e.currentTarget.value = ''
    }}
  />
</div>

<style>
  .editor {
    position: relative;
  }
  /* room for the toolbar fixed at the bottom on touch screens */
  @media (pointer: coarse) {
    .editor {
      padding-bottom: 72px;
    }
  }
  .surface {
    position: relative;
  }
  .image-bar {
    position: absolute;
    z-index: 5;
    display: flex;
    gap: 2px;
    padding: 2px;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-popover);
  }
</style>
