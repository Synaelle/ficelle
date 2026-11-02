<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Settings still being tried, each changing a lot about the server. For
     now the server for its admin alone, both ways: each asks first and
     says what changes. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api, type Overview } from './api.svelte'
  import { errorText } from './format'

  /** `onpersonal` hears whether the server is for its admin alone. */
  let { onpersonal }: { onpersonal?: (personal: boolean) => void } = $props()

  let overview: Overview | null = $state(null)
  let problem = $state('')

  async function load() {
    try {
      overview = await api<Overview>('GET', '/admin/overview')
      onpersonal?.(overview.personal)
    } catch (err) {
      problem = errorText(err)
    }
  }

  async function setPersonal(personal: boolean) {
    if (!overview) return
    problem = ''
    const question = personal
      ? t('admin.experimental.justMeConfirm', {
          others: overview.users - 1,
          signup: t(`admin.registration.${overview.registration}`).toLowerCase(),
        })
      : t('admin.overview.openToOthersConfirm')
    const yes = await overlay.ask(question, {
      confirmLabel: personal ? t('admin.overview.justMe') : t('admin.overview.openToOthers'),
    })
    if (!yes) return
    try {
      await api('PUT', '/admin/personal', { personal })
      await load()
    } catch (err) {
      problem = errorText(err)
    }
  }

  void load()
</script>

<p class="muted">{t('admin.experimental.help')}</p>
{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
{#if overview}
  <section class="setting">
    <h2>{t('admin.overview.justMe')}</h2>
    {#if overview.personal}
      <p>{t('admin.overview.personalHelp')}</p>
      <button class="btn" type="button" onclick={() => setPersonal(false)}
        >{t('admin.overview.openToOthers')}</button
      >
    {:else}
      <p>{t('admin.experimental.justMeHelp')}</p>
      <button class="btn" type="button" onclick={() => setPersonal(true)}
        >{t('admin.experimental.justMeButton')}</button
      >
    {/if}
  </section>
{/if}

<style>
  .setting {
    display: grid;
    gap: var(--space-3);
    justify-items: start;
    max-width: 620px;
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-left: 3px solid var(--color-warning);
    border-radius: var(--radius-popover);
  }
  h2 {
    margin: 0;
    font-size: 15px;
  }
  p {
    margin: 0;
  }
  p.muted {
    margin-bottom: var(--space-4);
  }
  .problem {
    color: var(--color-danger);
  }
</style>
