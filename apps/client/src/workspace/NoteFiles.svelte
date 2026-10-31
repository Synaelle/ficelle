<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The open note's attachments, folded at the very bottom of the note:
     each file's name, type and size, whether it is on this device, and a copy
     to save. Read again when the note or its files change. -->
<script lang="ts">
  import { onDestroy } from 'svelte'
  import { backend, type AttachmentFile } from '../lib/backend'
  import { bytes } from '../lib/format'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { pickToSave } from '../lib/pick'
  import { FILES_CHANGED } from '../lib/sync.svelte'
  import { isBackendError, workspace } from '../lib/workspace.svelte'

  let { noteId, body }: { noteId: string; body: string } = $props()

  let files: AttachmentFile[] = $state([])

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
  onDestroy(() => window.removeEventListener(FILES_CHANGED, again))

  const total = $derived(files.reduce((sum, f) => sum + f.size, 0))
  const kind = (file: AttachmentFile) =>
    file.fileName.includes('.') ? file.fileName.split('.').pop()?.toUpperCase() : file.mimeType

  async function saveCopy(file: AttachmentFile) {
    try {
      if (!file.here) await backend.fetchAttachment(file.sha256)
      const path = await pickToSave({ defaultPath: file.fileName })
      if (path) await backend.saveAttachment(file.sha256, path)
    } catch (err) {
      workspace.error = isBackendError(err) ? err.kind : 'unknown'
    }
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
            {kind(file)} · {bytes(file.size)}{#if file.refused}
              · {t(`errors.${file.refused}`)}{:else if !file.here}
              · {t('files.notHere')}{/if}
          </span>
          <button class="btn small" disabled={!backend.native} onclick={() => saveCopy(file)}
            >{t('files.saveCopy')}</button
          >
        </li>
      {/each}
    </ul>
  </details>
{/if}

<style>
  .files {
    margin-top: var(--space-7);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
  }
  summary {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-muted);
    cursor: pointer;
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
</style>
