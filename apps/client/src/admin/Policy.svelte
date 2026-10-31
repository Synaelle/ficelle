<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The files policy of one level, the instance, a group or a person: each
     rule either set here or inherited, with the value in effect and where it
     comes from. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { api, type FilesPolicy, type PolicySettings, type PolicyView } from './api.svelte'
  import { bytes, errorText } from './format'

  let {
    path,
    level,
    onsaved,
  }: {
    path: string
    level: 'instance' | 'group' | 'user'
    /** After a save, for what shows the policy's effects. */
    onsaved?: () => void
  } = $props()

  type Kind = 'mb' | 'count' | 'px' | 'quality' | 'days' | 'types' | 'bool'
  type Key = keyof PolicySettings
  /** Each rule: its setting, its value in effect, and how it reads. */
  const RULES: [Key, keyof FilesPolicy, Kind][] = [
    ['maxFileMb', 'maxFileBytes', 'mb'],
    ['quotaMb', 'quotaBytes', 'mb'],
    ['allowedTypes', 'allowedTypes', 'types'],
    ['blockedTypes', 'blockedTypes', 'types'],
    ['maxFilesPerNote', 'maxFilesPerNote', 'count'],
    ['imageMaxPx', 'imageMaxPx', 'px'],
    ['imageQuality', 'imageQuality', 'quality'],
    ['thumbnails', 'thumbnails', 'bool'],
    ['retentionDays', 'retentionDays', 'days'],
  ]
  const MB = 1024 * 1024

  let view: PolicyView | null = $state(null)
  let draft: PolicySettings = $state({})
  /** Text of the type lists while typed. */
  let lists: Record<string, string> = $state({})
  let problem = $state('')
  let saved = $state(false)

  async function load() {
    problem = ''
    try {
      view = await api<PolicyView>('GET', path)
      reset()
    } catch (err) {
      problem = errorText(err)
    }
  }

  function reset() {
    if (!view) return
    draft = structuredClone($state.snapshot(view.settings))
    lists = {
      allowedTypes: (draft.allowedTypes ?? []).join(', '),
      blockedTypes: (draft.blockedTypes ?? []).join(', '),
    }
  }

  /** The value in effect, as the setting's own unit. */
  function inEffect(current: PolicyView, key: Key, eff: keyof FilesPolicy) {
    const value = current.effective[eff]
    if (key === 'maxFileMb' || key === 'quotaMb') return Math.round(Number(value) / MB)
    return value
  }

  function toggle(current: PolicyView, key: Key, eff: keyof FilesPolicy, on: boolean) {
    saved = false
    if (!on) {
      draft = Object.fromEntries(
        Object.entries($state.snapshot(draft)).filter(([k]) => k !== key),
      ) as PolicySettings
      return
    }
    const value = structuredClone($state.snapshot(inEffect(current, key, eff)))
    draft = { ...draft, [key]: value }
    if (Array.isArray(value)) lists[key] = value.join(', ')
  }

  function shown(key: Key, kind: Kind, value: unknown): string {
    switch (kind) {
      case 'mb':
        return value === 0 ? t('admin.policy.noLimit') : bytes(Number(value))
      case 'count':
        return value === 0 ? t('admin.policy.noLimit') : String(value)
      case 'px':
        return value === 0
          ? t('admin.policy.keepImages')
          : t('admin.policy.pixels', { n: Number(value) })
      case 'quality':
        return `${value} %`
      case 'days':
        return t('admin.policy.days', { count: Number(value) })
      case 'bool':
        return value ? t('admin.account.on') : t('admin.account.off')
      case 'types': {
        const list = value as string[]
        if (list.length > 0) return list.join(', ')
        return t(key === 'allowedTypes' ? 'admin.policy.everyType' : 'admin.policy.none')
      }
    }
  }

  function source(current: PolicyView, eff: keyof FilesPolicy): string {
    const from = current.sources[eff]
    if (!from || from.level === level) return t('admin.policy.setHere')
    if (from.level === 'group') {
      const groups = from.groups ?? []
      return t('admin.policy.fromGroups', { count: groups.length, names: groups.join(', ') })
    }
    return t(`admin.policy.from.${from.level}`)
  }

  async function save(event: SubmitEvent) {
    event.preventDefault()
    problem = ''
    saved = false
    const body = { ...$state.snapshot(draft) } as Record<string, unknown>
    for (const key of ['allowedTypes', 'blockedTypes'] as const) {
      if (key in body)
        body[key] = (lists[key] ?? '')
          .split(/[\s,]+/)
          .map((s) => s.trim())
          .filter(Boolean)
    }
    try {
      view = await api<PolicyView>('PUT', path, body)
      reset()
      saved = true
      onsaved?.()
    } catch (err) {
      problem = errorText(err)
    }
  }

  $effect(() => {
    void path
    void load()
  })
</script>

{#if view}
  <form class="policy" onsubmit={save}>
    <p class="muted help">{t(`admin.policy.help.${level}`)}</p>
    {#each RULES as [key, eff, kind] (key)}
      {@const set = key in draft}
      <div class="rule">
        <label class="set">
          <input
            type="checkbox"
            checked={set}
            onchange={(e) => view && toggle(view, key, eff, e.currentTarget.checked)}
          />
          <span>{t(`admin.policy.keys.${key}`)}</span>
        </label>
        <div class="value">
          {#if set}
            {#if kind === 'bool'}
              <select
                class="field"
                aria-label={t(`admin.policy.keys.${key}`)}
                value={draft.thumbnails ? 'on' : 'off'}
                onchange={(e) => (draft = { ...draft, thumbnails: e.currentTarget.value === 'on' })}
              >
                <option value="on">{t('admin.account.on')}</option>
                <option value="off">{t('admin.account.off')}</option>
              </select>
            {:else if kind === 'types'}
              <input
                class="field"
                aria-label={t(`admin.policy.keys.${key}`)}
                placeholder=".pdf, image/*"
                bind:value={lists[key]}
              />
            {:else}
              <input
                class="field number"
                type="number"
                min={kind === 'quality' ? 1 : 0}
                max={kind === 'quality' ? 100 : undefined}
                aria-label={t(`admin.policy.keys.${key}`)}
                value={draft[key] as number}
                oninput={(e) =>
                  (draft = {
                    ...draft,
                    [key]: Math.max(0, Math.round(Number(e.currentTarget.value))),
                  })}
              />
              <span class="muted unit">{t(`admin.policy.units.${kind}`)}</span>
            {/if}
          {:else}
            <span>{shown(key, kind, view.effective[eff])}</span>
            <span class="muted from">{source(view, eff)}</span>
          {/if}
        </div>
      </div>
    {/each}
    {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
    <div class="actions">
      {#if saved}<span class="muted" role="status">{t('admin.policy.saved')}</span>{/if}
      <button class="btn" type="button" onclick={reset}>{t('admin.policy.undo')}</button>
      <button class="btn primary" type="submit">{t('admin.policy.save')}</button>
    </div>
  </form>
{:else if problem}
  <p class="problem" role="alert">{problem}</p>
{/if}

<style>
  .policy {
    display: grid;
    gap: var(--space-1);
    /* the person panel is narrow on any screen: the rules follow their own width */
    container-type: inline-size;
  }
  .help {
    margin: 0 0 var(--space-2);
  }
  .rule {
    display: grid;
    grid-template-columns: minmax(180px, 260px) minmax(0, 1fr);
    gap: var(--space-2) var(--space-3);
    align-items: center;
    min-height: 40px;
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--color-border);
  }
  .set {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .value {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-1) var(--space-3);
    min-width: 0;
  }
  .value input:not(.number) {
    flex: 1;
    min-width: 160px;
  }
  .number {
    width: 110px;
  }
  .value select {
    width: auto;
    min-width: 120px;
  }
  .from {
    font-size: 13px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
  .problem {
    color: var(--color-danger);
  }
  @container (max-width: 520px) {
    .rule {
      grid-template-columns: 1fr;
      gap: var(--space-1);
    }
  }
</style>
