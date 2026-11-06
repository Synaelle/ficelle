<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The server at a glance: its devices, security checklist and on or off
     settings. It is for one person; one with several accounts is pointed to
     going to one. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api, type Overview } from './api.svelte'
  import Checklist from './Checklist.svelte'
  import { errorText } from './format'

  let { several = false, ongo }: { several?: boolean; ongo?: () => void } = $props()

  let overview: Overview | null = $state(null)
  let problem = $state('')

  async function load() {
    try {
      overview = await api<Overview>('GET', '/admin/overview')
    } catch (err) {
      problem = errorText(err)
    }
  }

  /** Texts for a server of one person, who is its only admin. */
  const ownWords = 'Personal'

  async function setSecondStep(required: boolean, box: HTMLInputElement) {
    problem = ''
    // a password alone for every admin: said before it happens
    if (
      !required &&
      !(await overlay.ask(t(`admin.overview.secondStepOffConfirm${ownWords}`), {
        confirmLabel: t('admin.overview.secondStepOffYes'),
        danger: true,
      }))
    ) {
      box.checked = true
      return
    }
    try {
      await api('PUT', '/admin/second-step', { required })
      await load()
    } catch (err) {
      problem = errorText(err)
    }
  }

  async function setCoediting(on: boolean) {
    problem = ''
    try {
      await api('PUT', '/admin/coediting', { coediting: on })
      await load()
    } catch (err) {
      problem = errorText(err)
    }
  }

  void load()
</script>

{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
{#if several}
  <p class="warn" role="alert">
    {t('admin.onePerson.pointer')}
    <button class="btn" onclick={ongo}>{t('admin.onePerson.title')}</button>
  </p>
{/if}
{#if overview}
  <section class="personal">
    {#if !several}<p>{t('admin.overview.personalHelp')}</p>{/if}
    <dl class="counts">
      <div>
        <dt>{t('admin.overview.devices')}</dt>
        <dd>{overview.devices}</dd>
      </div>
    </dl>
  </section>
{/if}

<Checklist personal />

{#if overview}
  <!-- on or off settings, as switch rows -->
  <div class="switches">
    <label class="switch-row">
      <span>
        <b>{t(`admin.overview.secondStep${ownWords}`)}</b>
        <span class="muted">{t(`admin.overview.secondStepHelp${ownWords}`)}</span>
      </span>
      <input
        type="checkbox"
        role="switch"
        aria-label={t(`admin.overview.secondStepOn${ownWords}`)}
        checked={overview.adminsNeedSecondStep}
        onchange={(e) => setSecondStep(e.currentTarget.checked, e.currentTarget)}
      />
    </label>
    {#if !overview.adminsNeedSecondStep}
      <p class="warn" role="alert">{t(`admin.overview.secondStepOffRisks${ownWords}`)}</p>
    {/if}
    <label class="switch-row">
      <span>
        <b>{t('admin.overview.coediting')}</b>
        <span class="muted">{t(`admin.overview.coeditingHelp${ownWords}`)}</span>
      </span>
      <input
        type="checkbox"
        role="switch"
        aria-label={t('admin.overview.coeditingOn')}
        checked={overview.coediting}
        onchange={(e) => setCoediting(e.currentTarget.checked)}
      />
    </label>
  </div>

  <p class="muted small">
    {t('admin.overview.since', {
      date: overview.createdAt.slice(0, 10),
      version: overview.version,
    })}
  </p>
{/if}

<style>
  .personal {
    display: grid;
    gap: var(--space-3);
    justify-items: start;
    margin-bottom: var(--space-4);
  }
  .personal p {
    margin: 0;
  }
  .warn {
    margin: 0 0 var(--space-4);
    padding: var(--space-2) var(--space-3);
    border-left: 3px solid var(--color-danger);
  }
  /* the counts, small: a number over its name */
  .counts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
    gap: var(--space-2);
    margin: 0 0 var(--space-4);
  }
  .counts div {
    display: flex;
    flex-direction: column;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  dt {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  dd {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
  }
  .switch-row > span {
    display: grid;
    gap: 1px;
  }
  .switch-row .muted {
    font-size: 13px;
  }
  .switches {
    display: grid;
    gap: var(--space-2);
    max-width: 760px;
    margin-bottom: var(--space-4);
  }
  .switch-row {
    display: flex;
    gap: var(--space-4);
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    cursor: pointer;
  }
  /* a checkbox drawn as a switch */
  .switch-row input {
    appearance: none;
    flex: none;
    position: relative;
    width: 38px;
    height: 22px;
    margin: 0;
    border-radius: var(--radius-chip);
    background: var(--color-border);
    cursor: pointer;
    transition: background 0.15s;
  }
  .switch-row input::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: var(--radius-chip);
    background: var(--color-bg);
    transition: translate 0.15s;
  }
  .switch-row input:checked {
    background: var(--color-accent);
  }
  .switch-row input:checked::after {
    translate: 16px 0;
  }
  .switch-row input:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .small {
    font-size: 12px;
  }
  .problem {
    color: var(--color-danger);
  }
</style>
