<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The open note's attachments, folded at the very bottom of the note:
     each file's name, type and size, whether it is on this device, a copy to
     save, a mention to put in the text when it has none, and removal. Files
     added here stay out of the text until mentioned. Read again when the
     note or its files change. -->
<script lang="ts">
  import { onDestroy } from 'svelte'
  import { ATTACHMENT_PREFIX } from '../lib/attachments'
  import { backend, type AttachmentFile } from '../lib/backend'
  import { bytes } from '../lib/format'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { pickToSave } from '../lib/pick'
  import { FILES_CHANGED } from '../lib/sync.svelte'
  import { isBackendError, workspace } from '../lib/workspace.svelte'
  import { ATTACHMENTS_CHANGED } from '../editor/extensions/image'

  /** The open editor, absent while the note shows its markdown source. */
  interface EditorFiles {
    removeFile(file: AttachmentFile): Promise<void>
    mentionFile(file: AttachmentFile): void
    attachFiles(files: File[]): Promise<void>
  }

  let {
    noteId,
    body,
    editor,
    readonly = false,
  }: { noteId: string; body: string; editor?: EditorFiles; readonly?: boolean } = $props()

  let files: AttachmentFile[] = $state([])
  let picker: HTMLInputElement | undefined = $state()

  async function load(id: string) {
    try {
      files = await backend.attachmentFiles(id)
    } catch {
      files = []
    }
  }

  // the body is read too: a file pasted or removed changes it
  $effect(() => {
    void body
    void load(noteId)
  })
  const again = () => void load(noteId)
  window.addEventListener(FILES_CHANGED, again)
  window.addEventListener(ATTACHMENTS_CHANGED, again)
  onDestroy(() => {
    window.removeEventListener(FILES_CHANGED, again)
    window.removeEventListener(ATTACHMENTS_CHANGED, again)
  })

  const total = $derived(files.reduce((sum, f) => sum + f.size, 0))
  const kind = (file: AttachmentFile) =>
    file.fileName.includes('.') ? file.fileName.split('.').pop()?.toUpperCase() : file.mimeType
  /** Whether the text shows or links the file. */
  const mentioned = (file: AttachmentFile) =>
    body.includes(ATTACHMENT_PREFIX + encodeURI(file.fileName)) ||
    body.includes(ATTACHMENT_PREFIX + file.fileName)

  async function act(work: () => Promise<unknown>) {
    try {
      await work()
    } catch (err) {
      workspace.error = isBackendError(err) ? err.kind : 'unknown'
    }
    await load(noteId)
  }

  const saveCopy = (file: AttachmentFile) =>
    act(async () => {
      if (!file.here) await backend.fetchAttachment(file.sha256)
      const path = await pickToSave({ defaultPath: file.fileName })
      if (path) await backend.saveAttachment(file.sha256, path)
    })

  // the editor takes the file's mentions out of the text too
  const remove = (file: AttachmentFile) =>
    act(() => editor?.removeFile(file) ?? backend.removeAttachment(file.id))

  async function add(chosen: File[]) {
    if (chosen.length === 0) return
    await act(async () => {
      if (editor) await editor.attachFiles(chosen)
      else for (const file of chosen) await backend.addAttachment(noteId, file)
    })
  }
</script>

{#if files.length}
  <details class="files" data-note-files>
    <summary>
      <Icon name="attachment" size={14} />
      {t('files.inNote', { count: files.length })}
      <span class="muted">· {bytes(total)}</span>
    </summary>
    <ul>
      {#each files as file (file.id)}
        <li>
          <span class="name">{file.fileName}</span>
          <span class="muted">
            {[
              kind(file),
              bytes(file.size),
              file.refused ? t(`errors.${file.refused}`) : file.here ? '' : t('files.notHere'),
              mentioned(file) ? '' : t('files.notMentioned'),
            ]
              .filter(Boolean)
              .join(' · ')}
          </span>
          {#if editor && !readonly && !mentioned(file)}
            <button class="btn small" onclick={() => editor?.mentionFile(file)}
              >{t('files.mention')}</button
            >
          {/if}
          <button class="btn small" disabled={!backend.native} onclick={() => saveCopy(file)}
            >{t('files.saveCopy')}</button
          >
          {#if !readonly}
            <button
              class="icon-btn small danger"
              aria-label={t('files.remove')}
              title={t('files.remove')}
              onclick={() => remove(file)}><Icon name="trash" size={14} /></button
            >
          {/if}
        </li>
      {/each}
    </ul>
    {#if !readonly}
      <button class="btn small add" onclick={() => picker?.click()}
        ><Icon name="plus" size={14} />{t('files.attach')}</button
      >
    {/if}
  </details>
{:else if !readonly}
  <button class="attach" data-note-attach onclick={() => picker?.click()}
    ><Icon name="attachment" size={14} />{t('files.attach')}</button
  >
{/if}

{#if !readonly}
  <input
    bind:this={picker}
    type="file"
    multiple
    hidden
    onchange={(e) => {
      void add([...(e.currentTarget.files ?? [])])
      e.currentTarget.value = ''
    }}
  />
{/if}

<style>
  .files {
    margin-top: var(--space-7);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
  }
  summary,
  .attach {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-muted);
    cursor: pointer;
  }
  .attach {
    margin-top: var(--space-7);
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    font-size: 13px;
    opacity: 0.7;
  }
  .attach:hover,
  .attach:focus-visible {
    opacity: 1;
  }
  ul {
    display: grid;
    gap: var(--space-1);
    margin: var(--space-2) 0 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1) var(--space-3);
    padding: var(--space-1) 0;
    font-size: 13px;
  }
  .name {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .muted {
    flex: 1;
    color: var(--color-text-muted);
  }
  .small {
    padding: 2px var(--space-2);
    font-size: 12px;
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-top: var(--space-2);
  }
  .danger:hover {
    color: var(--color-danger);
  }
</style>
