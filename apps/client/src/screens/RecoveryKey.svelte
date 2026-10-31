<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Shows a new recovery key once, and only lets go once it is saved. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager'
  import { save } from '@tauri-apps/plugin-dialog'
  import { writeTextFile } from '@tauri-apps/plugin-fs'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'

  let {
    code,
    done,
    level = 'h2',
  }: {
    code: string
    done: () => void
    /** Heading level where it is shown: h1 alone on a screen, lower in a dialog. */
    level?: 'h1' | 'h2' | 'h3'
  } = $props()

  const groups = $derived(code.split('-'))
  const lines = $derived(
    Array.from({ length: Math.ceil(groups.length / 4) }, (_, i) => groups.slice(i * 4, i * 4 + 4)),
  )

  let saved = $state(false)
  let copied = $state(false)

  /** How long the key stays on the clipboard after a copy. */
  const CLIPBOARD_TIME = 60_000

  async function copy() {
    await writeText(code)
    copied = true
    // other apps read the clipboard: the key leaves it after a minute, if
    // nothing else was copied since
    setTimeout(() => {
      void readText()
        .then((now) => (now === code ? writeText('') : undefined))
        .catch(() => undefined)
    }, CLIPBOARD_TIME)
  }

  let savedFile = $state(false)
  let saveFailed = $state(false)
  /** A text file with the key and what it is, wherever the user picks. */
  async function saveFile() {
    saveFailed = false
    const name = t('recovery.fileName', { app: brand.displayName })
    const path = await save({
      defaultPath: `${name}.txt`,
      filters: [{ name: 'Text', extensions: ['txt'] }],
    })
    if (!path) return
    try {
      await writeTextFile(path, [name, '', code, '', t('recovery.fileNote'), ''].join('\n'))
      savedFile = true
    } catch {
      saveFailed = true
    }
  }
</script>

<div class="recovery">
  <svelte:element this={level} class="heading">{t('recovery.title')}</svelte:element>
  <p class="muted">{t('recovery.help')}</p>
  <!-- two even lines of four groups, easier to copy by hand than a wrap
       wherever the width ends, and a group never breaks in the middle -->
  <code aria-label={t('recovery.title')}
    >{#each lines as line, l (l)}<span class="line"
        >{#each line as group, i (i)}<span class="group"
            >{group}{l * 4 + i < groups.length - 1 ? '-' : ''}</span
          >{/each}</span
      >{/each}</code
  >
  <div class="keep">
    <button class="btn" type="button" onclick={copy}>
      {#if copied}<Icon name="check" size={16} />{/if}{copied
        ? t('recovery.copied')
        : t('recovery.copy')}
    </button>
    <button class="btn" type="button" onclick={saveFile}>
      {#if savedFile}<Icon name="check" size={16} />{/if}{savedFile
        ? t('recovery.savedFile')
        : t('recovery.saveFile')}
    </button>
  </div>
  {#if saveFailed}<p class="warning" role="alert">{t('recovery.saveFailed')}</p>{/if}
  <p class="warning">{t('recovery.warning')}</p>
  <label class="inline">
    <input type="checkbox" bind:checked={saved} />
    {t('recovery.saved')}
  </label>
  <button class="btn primary" type="button" disabled={!saved} onclick={done}
    >{t('recovery.continue')}</button
  >
</div>

<style>
  .recovery {
    display: grid;
    gap: var(--space-3);
  }
  .heading,
  p {
    margin: 0;
  }
  .heading {
    font-size: 18px;
  }
  code {
    padding: var(--space-3);
    border: 1px dashed var(--color-border-strong);
    border-radius: var(--radius-popover);
    background: var(--color-bg);
    font-family: var(--font-mono);
    font-size: 17px;
    letter-spacing: 0.04em;
    text-align: center;
    overflow-wrap: anywhere;
    user-select: all;
  }
  .keep {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2);
  }
  .line {
    display: block;
  }
  .group {
    display: inline-block;
  }
  .warning {
    color: var(--color-warning);
  }
  .inline {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
</style>
