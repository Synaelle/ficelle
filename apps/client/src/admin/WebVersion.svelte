<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The web version: off until turned on here, then who may open it, the
     sign-in limits and what it keeps. Who may open it and the limits are
     saved together from the bar at the bottom. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api, sectionHref, type SignInLimits, type WebAccess, type WebView } from './api.svelte'
  import { errorText } from './format'

  let { onchange }: { onchange?: (view: WebView) => void } = $props()

  type Open = Exclude<WebAccess, 'off'>
  type LimitKey = keyof SignInLimits

  let view = $state<WebView | null>(null)
  let problem = $state('')
  let busy = $state(false)
  let copied = $state(false)
  /** The choices on the page, saved by the bar. */
  let access: Open = $state('signedIn')
  let limits: SignInLimits = $state({
    perMinute: null,
    attempts: null,
    firstWait: null,
    longestWait: null,
  })

  const address = $derived(view?.address || new URL('web/', document.baseURI).href)
  const changes = $derived(
    !view
      ? 0
      : (view.access !== 'off' && view.access !== access ? 1 : 0) +
          (Object.keys(limits) as LimitKey[]).filter((k) => limits[k] !== view?.limits[k]).length,
  )

  function show(next: WebView) {
    view = next
    if (next.access !== 'off') access = next.access
    limits = { ...next.limits }
    onchange?.(next)
  }

  async function act(action: () => Promise<WebView>) {
    busy = true
    problem = ''
    try {
      show(await action())
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  async function openTo(next: Open) {
    if (
      next === 'anyone' &&
      !(await overlay.ask(t('admin.web.anyoneConfirm'), { confirmLabel: t('admin.web.anyoneYes') }))
    )
      return
    await act(() => api<WebView>('PUT', '/admin/web', { access: next }))
  }

  async function turnOff(box: HTMLInputElement) {
    if (
      !(await overlay.ask(t('admin.web.turnOffConfirm'), {
        confirmLabel: t('admin.account.turnOff'),
        danger: true,
      }))
    ) {
      box.checked = true
      return
    }
    await act(() => api<WebView>('PUT', '/admin/web', { access: 'off' }))
  }

  async function save() {
    if (!view) return
    const current = view
    if (current.access !== access) await openTo(access)
    if (problem) return
    await act(() => api<WebView>('PUT', '/admin/web/limits', limits))
  }

  function undo() {
    if (view) show(view)
  }

  async function copy() {
    await navigator.clipboard.writeText(address)
    copied = true
    setTimeout(() => (copied = false), 1500)
  }

  /** A number field: empty takes the config file's value. */
  function setLimit(key: LimitKey, raw: string) {
    const n = Number.parseInt(raw, 10)
    limits[key] = raw.trim() === '' || Number.isNaN(n) ? null : n
  }

  void act(() => api<WebView>('GET', '/admin/web'))
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

{#if problem}<p class="dash-problem" role="alert">{problem}</p>{/if}
{#if !view && !problem}<p class="muted" aria-busy="true">{t('admin.loading')}</p>{/if}

{#if view}
  <p class="dash-help">{t('admin.web.meta')}</p>

  {#if view.access === 'off'}
    <ul class="dash-rows">
      <li class="dash-row">
        <span class="dash-lead"><Icon name="globe" size={18} /></span>
        <span class="dash-words">
          <span class="dash-label">{t('admin.account.off')}</span>
          <span class="dash-help">{t('admin.web.offHelp')}</span>
        </span>
      </li>
    </ul>
    {#if view.files}
      <section class="dash-group" aria-labelledby="web-turn-on">
        <h2 id="web-turn-on">{t('admin.web.turnOnTitle')}</h2>
        {@render choices()}
        <p class="actions">
          <button class="btn primary" disabled={busy} onclick={() => openTo(access)}
            >{t('admin.web.turnOn')}</button
          >
          <span class="dash-help">{t('admin.web.turnOnLater')}</span>
        </p>
      </section>
    {:else}
      {@render missing()}
    {/if}
  {:else}
    <ul class="dash-rows">
      <li class="dash-row ok">
        <span class="dash-lead"><Icon name="globe" size={18} /></span>
        <span class="dash-words">
          <span class="dash-label">{t(`admin.web.onLabel_${view.access}`)}</span>
          <span class="dash-help mono">{address}</span>
        </span>
        <span class="dash-ctl">
          <button class="btn" onclick={copy}
            ><Icon name="copy" size={14} />{copied
              ? t('admin.copied')
              : t('admin.web.copy')}</button
          >
          <a class="btn" href={address} target="_blank" rel="noreferrer">{t('admin.web.open')}</a>
          <input
            class="dash-switch"
            type="checkbox"
            role="switch"
            aria-label={t('admin.web.turnOff')}
            checked
            disabled={busy}
            onchange={(e) => turnOff(e.currentTarget)}
          />
        </span>
      </li>
    </ul>

    <section class="dash-group" aria-labelledby="web-who">
      <h2 id="web-who">{t('admin.web.who')}</h2>
      {@render choices()}
    </section>

    {#if !view.files}{@render missing()}{/if}
  {/if}

  <section class="dash-group" aria-labelledby="web-limits">
    <h2 id="web-limits">
      {t('admin.web.limits')}
      <button
        class="btn quiet dash-aside"
        disabled={busy}
        onclick={() =>
          (limits = { perMinute: null, attempts: null, firstWait: null, longestWait: null })}
        >{t('admin.web.limitsDefault')}</button
      >
    </h2>
    <ul class="dash-rows">
      <li class="dash-row dash-tight">
        <span class="dash-words">
          <span class="dash-label">{t('admin.web.perMinute')}</span>
          <span class="dash-help">{t('admin.web.perMinuteHelp')}</span>
        </span>
        <span class="dash-ctl">{@render number('perMinute', t('admin.web.perMinute'))}</span>
      </li>
      <li class="dash-row dash-tight">
        <span class="dash-words">
          <span class="dash-label">{t('admin.web.attempts')}</span>
          <span class="dash-help">{t('admin.web.attemptsHelp')}</span>
        </span>
        <span class="dash-ctl">{@render number('attempts', t('admin.web.attempts'))}</span>
      </li>
      <li class="dash-row dash-tight">
        <span class="dash-words">
          <span class="dash-label">{t('admin.web.wait')}</span>
          <span class="dash-help">{t('admin.web.waitHelp')}</span>
        </span>
        <span class="dash-ctl">
          {@render number('firstWait', t('admin.web.firstWait'))}
          <span class="dash-unit">{t('admin.web.minUpTo')}</span>
          {@render number('longestWait', t('admin.web.longestWait'))}
          <span class="dash-unit">{t('admin.web.min')}</span>
        </span>
      </li>
    </ul>
    <p class="dash-help">{t('admin.web.limitsShared')}</p>
  </section>

  <section class="dash-group" aria-labelledby="web-keeps">
    <h2 id="web-keeps">{t('admin.web.keeps')}</h2>
    <ul class="dash-rows">
      <li class="dash-row dash-tight">
        <span class="dash-words">
          <span class="dash-label">{t('admin.web.notes')}</span>
          <span class="dash-help">{t('admin.web.notesHelp')}</span>
        </span>
      </li>
      <li class="dash-row dash-tight">
        <span class="dash-words"><span class="dash-label">{t('admin.web.files')}</span></span>
        <span class={['dash-val', view.files ? 'dash-ok' : 'dash-warn']}
          >{view.files ? t('admin.web.filesHere') : t('admin.web.filesMissing')}</span
        >
      </li>
      <li class="dash-row dash-tight">
        <span class="dash-words">
          <span class="dash-label">{t('admin.web.signIns')}</span>
          <span class="dash-help">{t('admin.web.signInsHelp')}</span>
        </span>
        <span class="dash-ctl">
          <span class="dash-val"
            >{t('admin.web.signInsValue', { count: view.signIns, refused: view.refused })}</span
          >
          <a class="btn quiet" href={sectionHref('security')}>{t('admin.web.log')}</a>
        </span>
      </li>
    </ul>
  </section>

  {#if changes > 0}
    <div class="dash-savebar">
      <span>{t('admin.unsaved', { count: changes })}</span>
      <button class="btn quiet" disabled={busy} onclick={undo}>{t('admin.undo')}</button>
      <button class="btn primary" disabled={busy} onclick={save}>{t('admin.save')}</button>
    </div>
  {/if}
{/if}

{#snippet choices()}
  <div class="dash-rows" role="radiogroup" aria-label={t('admin.web.who')}>
    {#each ['signedIn', 'anyone'] as const as option (option)}
      <label class="dash-choice">
        <input type="radio" name="web-access" value={option} bind:group={access} />
        <span class="dash-words">
          <span class="dash-label">{t(`admin.web.${option}`)}</span>
          <span class="dash-help">{t(`admin.web.${option}Help`)}</span>
        </span>
      </label>
    {/each}
  </div>
{/snippet}

{#snippet missing()}
  <ul class="dash-rows">
    <li class="dash-row warn">
      <span class="dash-lead"><Icon name="warning" size={16} /></span>
      <span class="dash-words">
        <span class="dash-label">{t('admin.web.filesMissing')}</span>
        <span class="dash-help">{t('admin.web.filesMissingHelp')}</span>
      </span>
    </li>
  </ul>
{/snippet}

<style>
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    margin: 0;
  }
  .mono {
    font-family: var(--font-mono);
  }
</style>
