<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The instance at a glance: its counts, security checklist, who may
     sign up and its on or off settings. A server for its admin alone shows
     only what is about one person; the switch between the two lives in
     Experimental. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api, type AccountKinds, type Overview, type Registration } from './api.svelte'
  import Checklist from './Checklist.svelte'
  import { errorText } from './format'

  /** `onpersonal` hears whether the server is for its admin alone. */
  let { onpersonal }: { onpersonal?: (personal: boolean) => void } = $props()

  let overview: Overview | null = $state(null)
  let problem = $state('')
  /** The radio picked, put back when opening sign-up is cancelled: the
   * browser keeps a clicked radio checked even when the saved mode stays. */
  const saved = (): Registration | undefined => overview?.registration
  let picked = $derived(saved())
  const MODES: Registration[] = ['closed', 'invite', 'open']
  const KINDS: AccountKinds[] = ['choice', 'e2ee', 'standard']

  async function load() {
    try {
      overview = await api<Overview>('GET', '/admin/overview')
      onpersonal?.(overview.personal)
    } catch (err) {
      problem = errorText(err)
    }
  }

  async function setMode(mode: Registration) {
    problem = ''
    // anyone on the internet, bots included: said before it happens
    if (
      mode === 'open' &&
      !(await overlay.ask(t('admin.registration.openConfirm'), {
        confirmLabel: t('admin.registration.openYes'),
        danger: true,
      }))
    ) {
      picked = saved()
      return
    }
    try {
      await api('PUT', '/admin/registration', { registration: mode })
      await load()
    } catch (err) {
      problem = errorText(err)
    }
  }

  async function setKinds(kinds: AccountKinds) {
    problem = ''
    try {
      await api('PUT', '/admin/account-kinds', { accountKinds: kinds })
      await load()
    } catch (err) {
      problem = errorText(err)
    }
  }

  /** Texts for a server of one person, who is its only admin. */
  const ownWords = $derived((overview as Overview | null)?.personal ? 'Personal' : '')

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
{#if overview?.personal}
  <section class="personal">
    <p>{t('admin.overview.personalHelp')}</p>
    <dl class="counts">
      <div>
        <dt>{t('admin.overview.devices')}</dt>
        <dd>{overview.devices}</dd>
      </div>
    </dl>
  </section>
{:else if overview}
  <dl class="counts">
    {#each [['users', overview.users], ['admins', overview.admins], ['e2ee', overview.e2ee], ['disabled', overview.disabled], ['groups', overview.groups], ['devices', overview.devices]] as const as [key, value] (key)}
      <div>
        <dd>{value}</dd>
        <dt>{t(`admin.overview.${key}`)}</dt>
      </div>
    {/each}
  </dl>
{/if}

<!-- read again when sign-up changes, which it names -->
{#key `${overview?.registration}${overview?.personal}`}<Checklist
    personal={overview?.personal}
  />{/key}

{#if overview && !overview.personal}
  <!-- who may join, two choices side by side on wide screens -->
  <div class="pair">
    <fieldset>
      <legend>{t('admin.overview.registration')}</legend>
      {#each MODES as mode (mode)}
        <label class="choice">
          <input
            type="radio"
            name="registration"
            value={mode}
            bind:group={picked}
            onchange={() => setMode(mode)}
          />
          <span>
            <b>{t(`admin.registration.${mode}`)}</b>
            <span class="muted">{t(`admin.registration.${mode}Help`)}</span>
          </span>
        </label>
      {/each}
      {#if overview.registration === 'open'}
        <p class="warn" role="alert">{t('admin.registration.openRisks')}</p>
      {/if}
    </fieldset>

    <fieldset>
      <legend>{t('admin.overview.accountKinds')}</legend>
      {#each KINDS as kinds (kinds)}
        <label class="choice">
          <input
            type="radio"
            name="account-kinds"
            checked={overview.accountKinds === kinds}
            onchange={() => setKinds(kinds)}
          />
          <span>
            <b>{t(`admin.accountKinds.${kinds}`)}</b>
            <span class="muted">{t(`admin.accountKinds.${kinds}Help`)}</span>
          </span>
        </label>
      {/each}
    </fieldset>
  </div>
{/if}
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
    margin: var(--space-1) 0 0;
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
  .pair {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: var(--space-5);
    margin: var(--space-2) 0 var(--space-4);
  }
  fieldset {
    display: grid;
    align-content: start;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: var(--space-2);
    font-weight: 600;
  }
  .choice {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
    cursor: pointer;
  }
  .choice input {
    margin-top: 3px;
  }
  .choice span,
  .switch-row > span {
    display: grid;
    gap: 1px;
  }
  .choice .muted,
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
    border-radius: 11px;
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
    border-radius: 50%;
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
