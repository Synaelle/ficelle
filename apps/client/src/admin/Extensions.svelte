<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Themes for the whole server: whether the apps may use any theme or only
     this server's, the ones this server offers, uploaded here, added on every
     device or only listed, and the official catalog, off until turned on
     here. The apps keep the rule. -->
<script lang="ts">
  import { validateTheme } from '@app/ui/theme'
  import { t } from '../lib/i18n.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api, type CatalogView, type ExtensionRules, type Extensions } from './api.svelte'
  import { bytes, errorText } from './format'

  let all: Extensions | null = $state(null)
  let rules: ExtensionRules = $state({ themes: 'any' })
  let problem = $state('')
  let saved = $state('')
  let busy = $state(false)
  let preinstallTheme = $state(true)
  let themeProblems: string[] = $state([])
  let catalog: CatalogView | null = $state(null)
  let catalogBusy = $state(false)

  async function setCatalog(enabled: boolean) {
    catalogBusy = true
    problem = ''
    try {
      // turning it on fetches it at once, so this is also "check again"
      catalog = await api<CatalogView>('PUT', '/admin/catalog', { enabled })
    } catch (err) {
      problem = errorText(err)
    } finally {
      catalogBusy = false
    }
  }

  const counts = $derived.by(() => ({
    themes: (catalog?.catalog?.entries ?? []).filter((e) => e.kind === 'theme').length,
  }))

  function show(next: Extensions) {
    all = next
    rules = structuredClone(next.rules)
  }

  async function run(action: () => Promise<Extensions>, done = '') {
    busy = true
    problem = ''
    saved = ''
    try {
      show(await action())
      saved = done
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  function saveRules() {
    void run(
      () => api<Extensions>('PUT', '/admin/extensions/rules', rules),
      t('admin.extensions.saved'),
    )
  }

  async function uploadTheme(input: HTMLInputElement) {
    const file = input.files?.[0]
    input.value = ''
    themeProblems = []
    if (!file) return
    const text = await file.text()
    let theme: unknown
    try {
      theme = JSON.parse(text)
    } catch {
      themeProblems = [t('settings.themeNotJson')]
      return
    }
    // the apps check it again before using it, this says why now
    const problems = validateTheme(theme)
    if (problems.length > 0) {
      themeProblems = problems
      return
    }
    await run(
      () =>
        api<Extensions>(
          'POST',
          `/admin/extensions/themes?preinstall=${preinstallTheme}`,
          new Blob([text], { type: 'application/json' }),
        ),
      t('admin.extensions.added', { name: file.name }),
    )
  }

  const setPreinstall = (id: string, preinstall: boolean) =>
    run(() => api<Extensions>('PATCH', `/admin/extensions/themes/${id}`, { preinstall }))

  async function remove(id: string, name: string) {
    const yes = await overlay.ask(t('admin.extensions.removeConfirm', { name }), {
      confirmLabel: t('admin.extensions.remove'),
      danger: true,
    })
    if (yes) await run(() => api<Extensions>('DELETE', `/admin/extensions/themes/${id}`))
  }

  void run(() => api<Extensions>('GET', '/admin/extensions'))
  api<CatalogView>('GET', '/admin/catalog')
    .then((view) => (catalog = view))
    .catch(() => {})
</script>

{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
{#if saved}<p class="saved" role="status">{saved}</p>{/if}

{#if all}
  <section>
    <h2>{t('admin.extensions.rules')}</h2>
    <p class="muted">{t('admin.extensions.rulesHelp')}</p>
    <fieldset>
      <legend>{t('admin.extensions.themesRule')}</legend>
      {#each ['any', 'instance'] as const as rule (rule)}
        <label class="choice">
          <input type="radio" name="themes" value={rule} bind:group={rules.themes} />
          {t(`admin.extensions.themes_${rule}`)}
        </label>
      {/each}
    </fieldset>
    <div class="actions">
      <button class="btn primary" disabled={busy} onclick={saveRules}
        >{t('admin.extensions.save')}</button
      >
    </div>
  </section>

  {#if catalog}
    <section data-catalog>
      <h2>{t('admin.extensions.catalog')}</h2>
      <p class="muted">{t('admin.extensions.catalogHelp')}</p>
      <label class="choice">
        <input
          type="checkbox"
          checked={catalog.enabled}
          disabled={catalogBusy}
          onchange={(e) => void setCatalog(e.currentTarget.checked)}
        />
        {t('admin.extensions.catalogOn')}
      </label>
      {#if catalog.enabled}
        {#if catalog.catalog}
          <p class="catalog-status">
            {t('admin.extensions.catalogCounts', counts)}{catalog.fetchedAt
              ? ` ${t('admin.extensions.catalogFetched', { when: new Date(catalog.fetchedAt).toLocaleString() })}`
              : ''}
          </p>
        {/if}
        {#if catalog.error}
          <p class="problem">{t('admin.extensions.catalogError', { error: catalog.error })}</p>
        {/if}
        <div class="actions">
          <button class="btn" disabled={catalogBusy} onclick={() => void setCatalog(true)}
            >{t('admin.extensions.catalogCheck')}</button
          >
        </div>
      {/if}
    </section>
  {/if}

  <section>
    <h2>{t('admin.extensions.themes')}</h2>
    <p class="muted">{t('admin.extensions.themesHelp')}</p>
    {#if all.themes.length === 0}
      <p class="muted">{t('admin.extensions.noThemes')}</p>
    {:else}
      <ul class="offered">
        {#each all.themes as theme (theme.id)}
          <li data-offered={theme.id}>
            <div class="head">
              <strong>{theme.name}</strong>
              <span class="muted">{theme.id}, {bytes(theme.size)}</span>
            </div>
            <div class="actions">
              <label class="choice">
                <input
                  type="checkbox"
                  checked={theme.preinstall}
                  disabled={busy}
                  onchange={(e) => void setPreinstall(theme.id, e.currentTarget.checked)}
                />
                {t('admin.extensions.preinstallTheme')}
              </label>
              <button
                class="btn danger"
                disabled={busy}
                onclick={() => void remove(theme.id, theme.name)}
                >{t('admin.extensions.remove')}</button
              >
            </div>
          </li>
        {/each}
      </ul>
    {/if}
    <div class="actions">
      <label class="btn upload">
        {t('admin.extensions.uploadTheme')}
        <input
          type="file"
          accept=".json,application/json"
          disabled={busy}
          onchange={(e) => void uploadTheme(e.currentTarget)}
        />
      </label>
      <label class="choice">
        <input type="checkbox" bind:checked={preinstallTheme} />
        {t('admin.extensions.preinstallTheme')}
      </label>
    </div>
    {#if themeProblems.length > 0}
      <div class="problem" role="alert">
        <p>{t('settings.themeInvalid')}</p>
        <ul>
          {#each themeProblems as p (p)}<li>{p}</li>{/each}
        </ul>
      </div>
    {/if}
  </section>
{/if}

<style>
  section {
    display: grid;
    gap: var(--space-3);
    margin-bottom: var(--space-6);
    justify-items: start;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  p {
    margin: 0;
  }
  fieldset {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: var(--space-1);
    font-weight: 600;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  .offered {
    justify-self: stretch;
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .offered > li {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2);
  }
  .head .muted {
    font-size: 12px;
  }
  .upload {
    position: relative;
    overflow: hidden;
    cursor: pointer;
  }
  .upload input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }
  .problem {
    color: var(--color-danger);
  }
  .problem ul {
    margin: var(--space-1) 0 0;
    padding-left: var(--space-5);
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .catalog-status {
    font-size: 13px;
  }
  .saved {
    color: var(--color-success);
  }
  .saved,
  .problem {
    margin-bottom: var(--space-4);
  }
</style>
