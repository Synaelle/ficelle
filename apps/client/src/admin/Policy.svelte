<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The files policy of one level, the instance, a group or a person: each
     rule either set here or inherited, with the value in effect and where it
     comes from. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
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
  /** Rules about images, in a group of their own. */
  const IMAGE_KINDS: Kind[] = ['px', 'quality', 'bool']

  let view: PolicyView | null = $state(null)
  let draft: PolicySettings = $state({})
  /** Text of the type lists while typed. */
  let lists: Record<string, string> = $state({})
  let problem = $state('')
  let saved = $state(false)
  /** Rules whose draft differs from what is saved. */
  const changes = $derived.by(() => {
    if (!view) return 0
    const before = view.settings as Record<string, unknown>
    const now = draft as Record<string, unknown>
    return RULES.filter(([key]) => {
      if (key === 'allowedTypes' || key === 'blockedTypes') {
        const typed = key in now ? (lists[key] ?? '') : undefined
        const was = key in before ? (before[key] as string[]).join(', ') : undefined
        return typed !== was
      }
      return JSON.stringify(now[key]) !== JSON.stringify(before[key])
    }).length
  })

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

  /** Where a value comes from, as an icon: the words go to its title. */
  function sourceIcon(current: PolicyView, eff: keyof FilesPolicy): IconName {
    const from = current.sources[eff]
    if (!from || from.level === level) return 'restore'
    return { config: 'configFile', instance: 'settings', group: 'groups', user: 'user' }[
      from.level
    ] as IconName
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

{#snippet rule([key, eff, kind]: [Key, keyof FilesPolicy, Kind], current: PolicyView)}
  {@const set = key in draft}
  {@const name = t(`admin.policy.keys.${key}`)}
  <li class="ad-row ad-tight">
    <span class="ad-words">
      <span class="ad-label">{name}</span>
      {#if set && (kind === 'mb' || kind === 'count' || kind === 'px')}
        <span class="ad-help">{t(`admin.policy.zero.${kind}`)}</span>
      {/if}
    </span>
    <span class="ad-val value">
      {#if !set}
        <button
          type="button"
          class="ad-val"
          aria-label={t('admin.policy.change', {
            rule: name,
            value: shown(key, kind, current.effective[eff]),
          })}
          onclick={() => toggle(current, key, eff, true)}
          >{shown(key, kind, current.effective[eff])}</button
        >
      {:else if kind === 'bool'}
        <input
          class="ad-switch"
          type="checkbox"
          role="switch"
          aria-label={name}
          checked={draft.thumbnails}
          onchange={(e) => (draft = { ...draft, thumbnails: e.currentTarget.checked })}
        />
      {:else if kind === 'types'}
        <input
          class="field edit types"
          aria-label={name}
          placeholder=".pdf, image/*"
          bind:value={lists[key]}
        />
      {:else}
        <input
          class="field edit"
          type="number"
          min={kind === 'quality' ? 1 : 0}
          max={kind === 'quality' ? 100 : undefined}
          aria-label={name}
          value={draft[key] as number}
          oninput={(e) =>
            (draft = {
              ...draft,
              [key]: Math.max(0, Math.round(Number(e.currentTarget.value))),
            })}
        />
        {#if kind !== 'count'}<span class="ad-unit unit">{t(`admin.policy.units.${kind}`)}</span
          >{/if}
      {/if}
    </span>
    <span class="ad-src">
      {#if set}
        <button
          type="button"
          class="icon-btn undo"
          aria-label={t('admin.policy.unset', { rule: name })}
          title={t('admin.policy.unset', { rule: name })}
          onclick={() => toggle(current, key, eff, false)}><Icon name="restore" size={14} /></button
        >
      {:else}
        <span class="from" title={source(current, eff)}
          ><Icon name={sourceIcon(current, eff)} size={14} /><span class="visually-hidden"
            >{source(current, eff)}</span
          ></span
        >
      {/if}
    </span>
  </li>
{/snippet}

{#if view}
  <form class="ad-page policy" onsubmit={save}>
    {#if level !== 'instance'}<p class="ad-help">{t(`admin.policy.help.${level}`)}</p>{/if}
    <section class="ad-group">
      <h2>
        {t('admin.policy.groupFiles')}
        <span class="ad-aside legend">
          {t('admin.policy.clickToChange')}
          <span><Icon name="configFile" size={14} />{t('admin.policy.legendConfig')}</span>
          <span class="here"><Icon name="restore" size={14} />{t('admin.policy.legendHere')}</span>
        </span>
      </h2>
      <ul class="ad-rows">
        {#each RULES.filter(([, , kind]) => !IMAGE_KINDS.includes(kind)) as item (item[0])}
          {@render rule(item, view)}
        {/each}
      </ul>
    </section>
    <section class="ad-group">
      <h2>{t('admin.policy.groupImages')}</h2>
      <ul class="ad-rows">
        {#each RULES.filter(([, , kind]) => IMAGE_KINDS.includes(kind)) as item (item[0])}
          {@render rule(item, view)}
        {/each}
      </ul>
    </section>
    {#if problem}<p class="ad-problem" role="alert">{problem}</p>{/if}
    {#if saved}<p class="ad-help" role="status">{t('admin.policy.saved')}</p>{/if}
    {#if changes > 0}
      <div class="ad-savebar">
        <span>{t('admin.unsaved', { count: changes })}</span>
        <button class="btn quiet" type="button" onclick={reset}>{t('admin.policy.undo')}</button>
        <button class="btn primary" type="submit">{t('admin.policy.save')}</button>
      </div>
    {/if}
  </form>
{:else if problem}
  <p class="ad-problem" role="alert">{problem}</p>
{/if}

<style>
  .policy {
    gap: var(--space-4);
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .legend .here {
    color: var(--color-accent-text);
  }
  /* one column for values, read or being changed, so nothing moves */
  .value {
    width: 190px;
    min-width: 0;
  }
  .value > .ad-val {
    min-width: 0;
    margin-right: -8px;
  }
  .edit {
    width: calc(6ch + 18px);
    height: 28px;
    padding: 0 var(--space-2);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .edit.types {
    width: 100%;
    text-align: left;
  }
  .unit {
    width: 4ch;
  }
  .ad-src {
    min-width: 32px;
  }
  .from {
    display: inline-flex;
  }
  .undo {
    width: 24px;
    height: 24px;
    color: var(--color-accent-text);
  }
  @media (max-width: 720px) {
    .value {
      width: auto;
    }
  }
</style>
