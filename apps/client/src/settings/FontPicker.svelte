<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The font of one part of the app (note text or code) over the theme's:
     the theme's own, one from the list (downloaded once when chosen), or one
     installed on the device. Code only offers fonts whose characters all
     have the same width. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { backend } from '../lib/backend'
  import {
    FONT_LIST,
    loadListedFont,
    type FontChoice,
    type FontKind,
    type FontRole,
  } from '../lib/fonts'
  import { t } from '../lib/i18n.svelte'
  import { prefs } from '../lib/prefs.svelte'

  let { role }: { role: FontRole } = $props()

  let device: { family: string; monospaced: boolean }[] = $state([])
  let busy = $state(false)
  let error = $state('')

  onMount(async () => {
    device = await backend.systemFonts().catch(() => [])
  })

  const key = $derived(role === 'notes' ? 'notesFont' : 'codeFont')
  const kinds: FontKind[] = $derived(
    role === 'code' ? ['mono'] : ['sans', 'serif', 'readable', 'mono'],
  )
  const deviceFonts = $derived(role === 'code' ? device.filter((f) => f.monospaced) : device)

  const valueOf = (choice: FontChoice | null) =>
    !choice ? '' : choice.source === 'list' ? `list:${choice.id}` : `device:${choice.family}`
  const value = $derived(valueOf(prefs[key]))

  async function choose(select: HTMLSelectElement) {
    const next = select.value
    error = ''
    if (!next) return void (prefs[key] = null)
    if (next.startsWith('device:'))
      return void (prefs[key] = { source: 'device', family: next.slice(7) })
    const id = next.slice(5)
    busy = true
    try {
      // kept only once its files are here, the text never shows a missing font
      if (await loadListedFont(id, true)) prefs[key] = { source: 'list', id }
    } catch {
      error = t('fonts.downloadFailed')
    } finally {
      // a font that did not come leaves the choice as it was
      select.value = valueOf(prefs[key])
      busy = false
    }
  }
</script>

<label class="font-picker">
  <span>{role === 'notes' ? t('fonts.notes') : t('fonts.code')}</span>
  <select class="field" disabled={busy} {value} onchange={(e) => void choose(e.currentTarget)}>
    <option value="">{t('fonts.theme')}</option>
    {#each kinds as kind (kind)}
      <optgroup label={t(`fonts.kind.${kind}`)}>
        {#each FONT_LIST.filter((f) => f.kind === kind) as font (font.id)}
          <option value="list:{font.id}">{font.family}</option>
        {/each}
      </optgroup>
    {/each}
    {#if deviceFonts.length}
      <optgroup label={t('fonts.device')}>
        {#each deviceFonts as font (font.family)}
          <option value="device:{font.family}">{font.family}</option>
        {/each}
      </optgroup>
    {/if}
  </select>
  {#if busy}<span class="status" role="status">{t('fonts.downloading')}</span>{/if}
  {#if error}<span class="error" role="alert">{error}</span>{/if}
</label>

<style>
  .font-picker {
    display: grid;
    gap: var(--space-1);
    width: min(320px, 100%);
  }
  .status {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .error {
    color: var(--color-danger);
    font-size: 13px;
  }
</style>
