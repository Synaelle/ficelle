<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- A server with several accounts going to one: pick the account to keep,
     type its name, the others go. Their notes stay on their own devices once
     their apps have updated, what the server held of them does not. Shown only
     while a server has more than one account. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { api, auth, signOut } from './api.svelte'
  import { date, errorText } from './format'

  let { ondone }: { ondone: () => void } = $props()

  interface Account {
    id: string
    username: string
    displayName: string
    lastSeenAt: number | null
    devices: number
    notes: number
  }

  let accounts = $state<Account[] | null>(null)
  let keep: string | null = $state(null)
  let typed = $state('')
  let busy = $state(false)
  let problem = $state('')

  const chosen = $derived(accounts?.find((a) => a.id === keep))
  const others = $derived((accounts?.length ?? 1) - 1)
  const ready = $derived(!!chosen && typed.trim() === chosen.username)

  async function load() {
    try {
      accounts = await api<Account[]>('GET', '/admin/one-person')
      keep = accounts.find((a) => a.id === auth.session?.user.id)?.id ?? null
    } catch (err) {
      problem = errorText(err)
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    if (!chosen || !ready) return
    busy = true
    problem = ''
    try {
      await api('POST', '/admin/one-person', { keep: chosen.id, confirm: typed.trim() })
      // another account was kept: this one is gone, its session with it
      if (chosen.id !== auth.session?.user.id) await signOut()
      else ondone()
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  void load()
</script>

<form class="one" onsubmit={submit}>
  <p>{t('admin.onePerson.help')}</p>
  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
  {#if accounts}
    <fieldset>
      <legend>{t('admin.onePerson.choose')}</legend>
      {#each accounts as account (account.id)}
        <label class="account" class:picked={keep === account.id}>
          <input type="radio" name="keep" value={account.id} bind:group={keep} />
          <span class="words">
            <b>{account.displayName || account.username}</b>
            <span class="muted">
              {account.username} ·
              {t('admin.onePerson.notes', { count: account.notes })} ·
              {t('admin.onePerson.devices', { count: account.devices })}
              {#if account.lastSeenAt}· {t('admin.onePerson.seen', {
                  date: date(account.lastSeenAt),
                })}{/if}
            </span>
          </span>
        </label>
      {/each}
    </fieldset>
    {#if chosen}
      <p class="warn" role="alert">
        {t('admin.onePerson.warning', { count: others, name: chosen.username })}
      </p>
      <label class="field-label">
        {t('admin.onePerson.typeName', { name: chosen.username })}
        <input class="field" autocomplete="off" bind:value={typed} />
      </label>
      <button class="btn danger" type="submit" disabled={busy || !ready}
        >{t('admin.onePerson.submit', { count: others })}</button
      >
    {/if}
  {/if}
</form>

<style>
  .one {
    display: grid;
    gap: var(--space-3);
    justify-items: start;
    max-width: 640px;
  }
  p {
    margin: 0;
  }
  fieldset {
    display: grid;
    gap: var(--space-2);
    justify-self: stretch;
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: var(--space-2);
    font-weight: 600;
  }
  .account {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    cursor: pointer;
  }
  .account.picked {
    border-color: var(--color-accent);
  }
  .account input {
    margin-top: 3px;
  }
  .words {
    display: grid;
    gap: 1px;
  }
  .words .muted {
    font-size: 13px;
  }
  .warn {
    padding: var(--space-2) var(--space-3);
    border-left: 3px solid var(--color-danger);
  }
  .field-label {
    display: grid;
    gap: var(--space-1);
    justify-self: stretch;
  }
  .problem {
    color: var(--color-danger);
  }
</style>
