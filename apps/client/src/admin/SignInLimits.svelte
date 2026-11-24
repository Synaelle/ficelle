<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The sign-in limits, for every way of signing in. An empty field takes
     the config file's value, the bar at the bottom saves them. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { api, type SignInLimits, type SignInLimitsView } from './api.svelte'
  import { errorText } from './format'

  type LimitKey = keyof SignInLimits

  let view = $state<SignInLimitsView | null>(null)
  let problem = $state('')
  let busy = $state(false)
  let limits: SignInLimits = $state({
    perMinute: null,
    attempts: null,
    firstWait: null,
    longestWait: null,
  })

  const changes = $derived(
    !view
      ? 0
      : (Object.keys(limits) as LimitKey[]).filter((k) => limits[k] !== view?.limits[k]).length,
  )

  async function act(action: () => Promise<SignInLimitsView>) {
    busy = true
    problem = ''
    try {
      view = await action()
      limits = { ...view.limits }
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  function undo() {
    if (view) limits = { ...view.limits }
  }

  /** A number field: empty takes the config file's value. */
  function setLimit(key: LimitKey, raw: string) {
    const n = Number.parseInt(raw, 10)
    limits[key] = raw.trim() === '' || Number.isNaN(n) ? null : n
  }

  void act(() => api<SignInLimitsView>('GET', '/admin/signin-limits'))
</script>

{#snippet number(key: LimitKey, label: string)}
  <input
    class="field dash-num"
    type="number"
    min="1"
    inputmode="numeric"
    aria-label={label}
    placeholder={String(view?.defaults[key] ?? '')}
    value={limits[key] ?? ''}
    oninput={(e) => setLimit(key, e.currentTarget.value)}
  />
{/snippet}

<section class="dash-group" aria-labelledby="signin-limits">
  <h2 id="signin-limits">
    {t('admin.limits.title')}
    <button
      class="btn quiet dash-aside"
      disabled={busy || !view}
      onclick={() =>
        (limits = { perMinute: null, attempts: null, firstWait: null, longestWait: null })}
      >{t('admin.limits.default')}</button
    >
  </h2>
  {#if problem}<p class="dash-problem" role="alert">{problem}</p>{/if}
  {#if !view && !problem}<p class="muted" aria-busy="true">{t('admin.loading')}</p>{/if}
  {#if view}
    <ul class="dash-rows">
      <li class="dash-row dash-tight">
        <span class="dash-words">
          <span class="dash-label">{t('admin.limits.perMinute')}</span>
        </span>
        <span class="dash-ctl">{@render number('perMinute', t('admin.limits.perMinute'))}</span>
      </li>
      <li class="dash-row dash-tight">
        <span class="dash-words">
          <span class="dash-label">{t('admin.limits.attempts')}</span>
        </span>
        <span class="dash-ctl">{@render number('attempts', t('admin.limits.attempts'))}</span>
      </li>
      <li class="dash-row dash-tight">
        <span class="dash-words">
          <span class="dash-label">{t('admin.limits.wait')}</span>
        </span>
        <span class="dash-ctl">
          {@render number('firstWait', t('admin.limits.firstWait'))}
          <span class="dash-unit">{t('admin.limits.minUpTo')}</span>
          {@render number('longestWait', t('admin.limits.longestWait'))}
          <span class="dash-unit">{t('admin.limits.min')}</span>
        </span>
      </li>
    </ul>
  {/if}
</section>

{#if changes > 0}
  <div class="dash-savebar">
    <span>{t('admin.unsaved', { count: changes })}</span>
    <button class="btn quiet" disabled={busy} onclick={undo}>{t('admin.undo')}</button>
    <button
      class="btn primary"
      disabled={busy}
      onclick={() => act(() => api<SignInLimitsView>('PUT', '/admin/signin-limits', limits))}
      >{t('admin.save')}</button
    >
  </div>
{/if}
