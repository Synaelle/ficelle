<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The instance at a glance: its security checklist, counts, and who may
     sign up. A server for its admin alone shows only what is about one
     person, and can be opened to others. -->
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

  async function setPersonal(personal: boolean) {
    problem = ''
    if (
      !personal &&
      !(await overlay.ask(t('admin.overview.openToOthersConfirm'), {
        confirmLabel: t('admin.overview.openToOthers'),
      }))
    )
      return
    try {
      await api('PUT', '/admin/personal', { personal })
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

<!-- read again when sign-up changes, which it names -->
{#key `${overview?.registration}${overview?.personal}`}<Checklist
    personal={overview?.personal}
  />{/key}
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
    <button class="btn" type="button" onclick={() => setPersonal(false)}
      >{t('admin.overview.openToOthers')}</button
    >
  </section>
{:else if overview}
  <dl class="counts">
    {#each [['users', overview.users], ['admins', overview.admins], ['e2ee', overview.e2ee], ['disabled', overview.disabled], ['groups', overview.groups], ['devices', overview.devices]] as const as [key, value] (key)}
      <div>
        <dt>{t(`admin.overview.${key}`)}</dt>
        <dd>{value}</dd>
      </div>
    {/each}
  </dl>

  <fieldset>
    <legend>{t('admin.overview.registration')}</legend>
    {#each MODES as mode (mode)}
      <label class="mode" class:checked={overview.registration === mode}>
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
      <label class="mode" class:checked={overview.accountKinds === kinds}>
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

  <button class="link" type="button" onclick={() => setPersonal(true)}
    >{t('admin.overview.justMe')}</button
  >
{/if}
{#if overview}
  <fieldset>
    <legend>{t(`admin.overview.secondStep${ownWords}`)}</legend>
    <label class="mode" class:checked={overview.adminsNeedSecondStep}>
      <input
        type="checkbox"
        checked={overview.adminsNeedSecondStep}
        onchange={(e) => setSecondStep(e.currentTarget.checked, e.currentTarget)}
      />
      <span>
        <b>{t(`admin.overview.secondStepOn${ownWords}`)}</b>
        <span class="muted">{t(`admin.overview.secondStepHelp${ownWords}`)}</span>
      </span>
    </label>
    {#if !overview.adminsNeedSecondStep}
      <p class="warn" role="alert">{t(`admin.overview.secondStepOffRisks${ownWords}`)}</p>
    {/if}
  </fieldset>

  <fieldset>
    <legend>{t('admin.overview.coediting')}</legend>
    <label class="mode" class:checked={overview.coediting}>
      <input
        type="checkbox"
        checked={overview.coediting}
        onchange={(e) => setCoediting(e.currentTarget.checked)}
      />
      <span>
        <b>{t('admin.overview.coeditingOn')}</b>
        <span class="muted">{t(`admin.overview.coeditingHelp${ownWords}`)}</span>
      </span>
    </label>
  </fieldset>

  <p class="muted">
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
    margin-bottom: var(--space-5);
  }
  .personal p {
    margin: 0;
  }
  .link {
    padding: 0;
    margin-bottom: var(--space-5);
    border: 0;
    background: none;
    color: var(--color-accent);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    text-decoration: underline;
  }
  .warn {
    margin: var(--space-2) 0 0;
    padding: var(--space-2) var(--space-3);
    border-left: 3px solid var(--color-danger);
  }
  .counts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: var(--space-3);
    margin: 0 0 var(--space-5);
  }
  .counts div {
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  dt {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  dd {
    margin: var(--space-1) 0 0;
    font-size: 26px;
    font-weight: 600;
  }
  fieldset {
    display: grid;
    gap: var(--space-2);
    max-width: 620px;
    margin: 0 0 var(--space-4);
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: var(--space-2);
    font-weight: 600;
  }
  .mode {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    cursor: pointer;
  }
  .mode.checked {
    border-color: var(--color-accent);
  }
  .mode span {
    display: grid;
    gap: 2px;
  }
  .problem {
    color: var(--color-danger);
  }
</style>
